import fs from "node:fs";

export const DEFAULT_BLOCK_WARNING_MS = 2_000;
export const DEFAULT_BLOCK_FAILURE_MS = 4_000;
export const DEFAULT_HEAD_WARNING_MS = 4_000;
export const DEFAULT_HEAD_VIOLATION_MS = 12_000;
export const DEFAULT_HEAD_TIMEOUT_MS = 12_000;
export const COLLECT_HEAD_TIMEOUT_MS = 120_000;
export const DEFAULT_MIN_BLOCK_SAMPLES = 20;
export const DEFAULT_SAMPLE_DRAIN_TIMEOUT_MS = 120_000;
export const ACCELERATED_SEALING_MS = 250;

export function remainingRequiredBlockSamples(
  observedSamples: number,
  minimumSamples = DEFAULT_MIN_BLOCK_SAMPLES,
): number {
  if (!Number.isInteger(observedSamples) || observedSamples < 0) {
    throw new Error(`invalid observed block samples: ${observedSamples}`);
  }
  if (!Number.isInteger(minimumSamples) || minimumSamples < 1) {
    throw new Error(`invalid minimum block samples: ${minimumSamples}`);
  }
  return Math.max(0, minimumSamples - observedSamples);
}

export function sampleDrainFailureReason(
  observedSamples: number,
  elapsedMs: number,
  minimumSamples = DEFAULT_MIN_BLOCK_SAMPLES,
  timeoutMs = DEFAULT_SAMPLE_DRAIN_TIMEOUT_MS,
): string | undefined {
  const missingSamples = remainingRequiredBlockSamples(observedSamples, minimumSamples);
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) {
    throw new Error(`invalid sample drain elapsed time: ${elapsedMs}`);
  }
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`invalid sample drain timeout: ${timeoutMs}`);
  }
  if (missingSamples === 0 || elapsedMs < timeoutMs) {
    return undefined;
  }
  return (
    `expected proposer log format produced ${observedSamples} sample(s); ` +
    `required ${minimumSamples} within ${timeoutMs}ms after the workload completed`
  );
}

export interface BlockConstructionSample {
  block: number;
  durationMs: number;
}

export interface ParsedBlockChunk {
  remainder: string;
  samples: BlockConstructionSample[];
}

export interface BlockLatencySummary {
  sampleCount: number;
  meanMs: number | null;
  maximumMs: number | null;
  p50Ms: number | null;
  p95Ms: number | null;
  p99Ms: number | null;
  warnings: BlockConstructionSample[];
  violations: BlockConstructionSample[];
  slowestBlocks: BlockConstructionSample[];
}

export interface BestHeadStall {
  afterBlock: number;
  warnedAtMs: number;
  violatedAtMs: number | null;
  endedAtMs: number | null;
  outcome: "active" | "recovered" | "aborted";
  durationMs: number;
}

export type LivenessTick =
  | { kind: "healthy" }
  | { kind: "warning"; stall: BestHeadStall }
  | { kind: "violation"; stall: BestHeadStall }
  | { kind: "abort"; stall: BestHeadStall };

export interface EpochBaseline {
  netuid: number;
  tempo: number;
  epochIndex: bigint;
}

export interface EpochProgress extends EpochBaseline {
  currentEpochIndex: bigint;
  completedCycles: bigint;
}

export interface EpochCoverageEvaluation {
  complete: boolean;
  progress: EpochProgress[];
  removedNetuids: number[];
  missingNetuids: number[];
  regressedNetuids: number[];
}

export interface EpochCoverageBudget {
  activeSubnets: number;
  maxTempo: number;
  maxEpochsPerBlock: number;
  schedulingBlocksPerCycle: number;
  unpaddedBlocks: number;
  blockBudget: number;
  nominalWallMs: number;
}

export class NodeLogTail {
  private position: number;
  private remainder = "";

  constructor(
    private readonly filename: string,
    startOffset: number,
  ) {
    const size = fs.statSync(filename).size;
    if (startOffset < 0 || startOffset > size) {
      throw new Error(`invalid node log offset ${startOffset}; ${filename} is ${size} bytes`);
    }
    this.position = startOffset;
  }

  read(flush = false): BlockConstructionSample[] {
    const size = fs.statSync(this.filename).size;
    if (size < this.position) {
      throw new Error(`node log was truncated from ${this.position} to ${size} bytes`);
    }

    let chunk = "";
    if (size > this.position) {
      const length = size - this.position;
      const buffer = Buffer.alloc(length);
      const descriptor = fs.openSync(this.filename, "r");
      try {
        fs.readSync(descriptor, buffer, 0, length, this.position);
      } finally {
        fs.closeSync(descriptor);
      }
      this.position = size;
      chunk = buffer.toString("utf8");
    }

    const parsed = parsePreparedBlockChunk(this.remainder, chunk, flush);
    this.remainder = parsed.remainder;
    return parsed.samples;
  }
}

const PREPARED_BLOCK_PATTERN = /Prepared block for proposing at\s+#?(\d+)\s+\((\d+)\s*ms\)/;

export function parsePreparedBlockChunk(
  remainder: string,
  chunk: string,
  flush = false,
): ParsedBlockChunk {
  const lines = `${remainder}${chunk}`.split(/\r?\n/);
  const nextRemainder = flush ? "" : (lines.pop() ?? "");
  const samples: BlockConstructionSample[] = [];

  for (const line of lines) {
    const match = PREPARED_BLOCK_PATTERN.exec(line);
    if (match === null) {
      continue;
    }
    samples.push({
      block: Number.parseInt(match[1], 10),
      durationMs: Number.parseInt(match[2], 10),
    });
  }

  return { remainder: nextRemainder, samples };
}

export function summarizeBlockSamples(
  samples: readonly BlockConstructionSample[],
  warningMs = DEFAULT_BLOCK_WARNING_MS,
  failureMs = DEFAULT_BLOCK_FAILURE_MS,
): BlockLatencySummary {
  const byDuration = [...samples].sort((left, right) => left.durationMs - right.durationMs);
  const warnings = samples.filter(
    ({ durationMs }) => durationMs >= warningMs && durationMs < failureMs,
  );
  const violations = samples.filter(({ durationMs }) => durationMs >= failureMs);
  const slowestBlocks = [...samples]
    .sort((left, right) => right.durationMs - left.durationMs || left.block - right.block)
    .slice(0, 20);

  return {
    sampleCount: samples.length,
    meanMs:
      samples.length === 0
        ? null
        : Math.round(
            (samples.reduce((total, { durationMs }) => total + durationMs, 0) / samples.length) *
              10,
          ) / 10,
    maximumMs: byDuration.at(-1)?.durationMs ?? null,
    p50Ms: percentile(byDuration, 50),
    p95Ms: percentile(byDuration, 95),
    p99Ms: percentile(byDuration, 99),
    warnings,
    violations,
    slowestBlocks,
  };
}

export function blockLatencyFailureReasons(
  summary: BlockLatencySummary,
  minimumSamples: number,
  failureMs = DEFAULT_BLOCK_FAILURE_MS,
): string[] {
  const reasons: string[] = [];
  const missingSamples = remainingRequiredBlockSamples(summary.sampleCount, minimumSamples);
  if (missingSamples > 0) {
    reasons.push(`observed ${summary.sampleCount} proposer samples; required at least ${minimumSamples}`);
  }
  if (summary.violations.length > 0) {
    reasons.push(`${summary.violations.length} block(s) met or exceeded ${failureMs}ms`);
  }
  return reasons;
}

export class BestHeadLiveness {
  private lastBlock: number | undefined;
  private lastObservedAtMs: number | undefined;
  private activeStall: BestHeadStall | undefined;
  private readonly completedStalls: BestHeadStall[] = [];

  constructor(
    readonly warningMs = DEFAULT_HEAD_WARNING_MS,
    readonly violationMs = DEFAULT_HEAD_VIOLATION_MS,
    readonly timeoutMs = DEFAULT_HEAD_TIMEOUT_MS,
  ) {
    if (!(warningMs > 0 && violationMs > warningMs && timeoutMs >= violationMs)) {
      throw new Error(
        `invalid best-head thresholds: warning=${warningMs} violation=${violationMs} ` +
          `timeout=${timeoutMs}`,
      );
    }
  }

  observe(block: number, nowMs: number): BestHeadStall | undefined {
    if (this.lastBlock !== undefined && block < this.lastBlock) {
      throw new Error(`best head regressed from ${this.lastBlock} to ${block}`);
    }
    if (this.lastBlock === block) {
      return undefined;
    }

    let recovered: BestHeadStall | undefined;
    if (this.activeStall !== undefined && this.lastObservedAtMs !== undefined) {
      this.activeStall.endedAtMs = nowMs;
      this.activeStall.outcome = "recovered";
      this.activeStall.durationMs = nowMs - this.lastObservedAtMs;
      recovered = { ...this.activeStall };
      this.completedStalls.push(recovered);
      this.activeStall = undefined;
    }

    this.lastBlock = block;
    this.lastObservedAtMs = nowMs;
    return recovered;
  }

  tick(nowMs: number): LivenessTick {
    if (this.lastBlock === undefined || this.lastObservedAtMs === undefined) {
      return { kind: "healthy" };
    }

    const durationMs = nowMs - this.lastObservedAtMs;
    if (durationMs < this.warningMs) {
      return { kind: "healthy" };
    }

    if (this.activeStall === undefined) {
      this.activeStall = {
        afterBlock: this.lastBlock,
        warnedAtMs: nowMs,
        violatedAtMs: null,
        endedAtMs: null,
        outcome: "active",
        durationMs,
      };
      if (durationMs < this.violationMs) {
        return { kind: "warning", stall: { ...this.activeStall } };
      }
    } else {
      this.activeStall.durationMs = durationMs;
    }

    if (durationMs >= this.timeoutMs) {
      this.activeStall.endedAtMs = nowMs;
      this.activeStall.outcome = "aborted";
      return { kind: "abort", stall: { ...this.activeStall } };
    }
    if (durationMs >= this.violationMs && this.activeStall.violatedAtMs === null) {
      this.activeStall.violatedAtMs = nowMs;
      return { kind: "violation", stall: { ...this.activeStall } };
    }
    return { kind: "healthy" };
  }

  getLastBlock(): number | undefined {
    return this.lastBlock;
  }

  getStalls(nowMs = Date.now()): BestHeadStall[] {
    const result = this.completedStalls.map((stall) => ({ ...stall }));
    if (this.activeStall !== undefined && this.lastObservedAtMs !== undefined) {
      result.push({
        ...this.activeStall,
        durationMs: Math.max(this.activeStall.durationMs, nowMs - this.lastObservedAtMs),
      });
    }
    return result;
  }
}

export function bestHeadFailureReasons(
  stalls: readonly BestHeadStall[],
  violationMs = DEFAULT_HEAD_VIOLATION_MS,
): string[] {
  const violations = stalls.filter(({ durationMs }) => durationMs >= violationMs);
  return violations.length === 0
    ? []
    : [`${violations.length} best-head stall(s) met or exceeded ${violationMs}ms`];
}

export function evaluateEpochCoverage(
  baseline: readonly EpochBaseline[],
  currentEpochIndices: ReadonlyMap<number, bigint>,
  activeNetuids: ReadonlySet<number>,
  requiredCycles: number,
): EpochCoverageEvaluation {
  if (!Number.isInteger(requiredCycles) || requiredCycles < 1) {
    throw new Error(`invalid required epoch cycles: ${requiredCycles}`);
  }

  const progress: EpochProgress[] = [];
  const removedNetuids: number[] = [];
  const missingNetuids: number[] = [];
  const regressedNetuids: number[] = [];

  for (const subnet of baseline) {
    if (!activeNetuids.has(subnet.netuid)) {
      removedNetuids.push(subnet.netuid);
    }
    const currentEpochIndex = currentEpochIndices.get(subnet.netuid);
    if (currentEpochIndex === undefined) {
      missingNetuids.push(subnet.netuid);
      continue;
    }
    if (currentEpochIndex < subnet.epochIndex) {
      regressedNetuids.push(subnet.netuid);
    }
    progress.push({
      ...subnet,
      currentEpochIndex,
      completedCycles:
        currentEpochIndex >= subnet.epochIndex ? currentEpochIndex - subnet.epochIndex : 0n,
    });
  }

  return {
    complete:
      removedNetuids.length === 0 &&
      missingNetuids.length === 0 &&
      regressedNetuids.length === 0 &&
      progress.length === baseline.length &&
      progress.every(({ completedCycles }) => completedCycles >= BigInt(requiredCycles)),
    progress,
    removedNetuids,
    missingNetuids,
    regressedNetuids,
  };
}

export function computeEpochCoverageBudget(
  baseline: readonly EpochBaseline[],
  requiredCycles: number,
  maxEpochsPerBlock: number,
  margin = 0.1,
): EpochCoverageBudget {
  if (baseline.length === 0) {
    throw new Error("cannot calculate epoch coverage for an empty subnet baseline");
  }
  if (!Number.isInteger(requiredCycles) || requiredCycles < 1) {
    throw new Error(`invalid required epoch cycles: ${requiredCycles}`);
  }
  if (!Number.isInteger(maxEpochsPerBlock) || maxEpochsPerBlock < 1) {
    throw new Error(`invalid MaxEpochsPerBlock: ${maxEpochsPerBlock}`);
  }
  if (!(margin >= 0 && margin <= 1)) {
    throw new Error(`invalid epoch coverage margin: ${margin}`);
  }
  for (const { netuid, tempo } of baseline) {
    if (!Number.isInteger(tempo) || tempo < 1) {
      throw new Error(`invalid tempo ${tempo} for subnet ${netuid}`);
    }
  }

  const maxTempo = Math.max(...baseline.map(({ tempo }) => tempo));
  const schedulingBlocksPerCycle = Math.ceil(baseline.length / maxEpochsPerBlock);
  const unpaddedBlocks = requiredCycles * (maxTempo + schedulingBlocksPerCycle);
  const blockBudget = Math.ceil(unpaddedBlocks * (1 + margin));

  return {
    activeSubnets: baseline.length,
    maxTempo,
    maxEpochsPerBlock,
    schedulingBlocksPerCycle,
    unpaddedBlocks,
    blockBudget,
    nominalWallMs: blockBudget * ACCELERATED_SEALING_MS,
  };
}

function percentile(sorted: readonly BlockConstructionSample[], value: number): number | null {
  if (sorted.length === 0) {
    return null;
  }
  const index = Math.max(0, Math.ceil((value / 100) * sorted.length) - 1);
  return sorted[index].durationMs;
}
