import styles from '../v436-upgrade/page.module.css';

const GRAPH_TEXT = {
  fontFamily: 'FiraCode',
  fontSize: 10,
  fill: 'rgb(41, 41, 41)',
} as const;

const INK = 'rgb(41, 41, 41)';
const MUTED = 'rgba(41, 41, 41, 0.5)';
const FAINT = 'rgba(41, 41, 41, 0.12)';
const ACCENT = '#d15168';
const ACCENT_SOFT = 'rgba(209, 81, 104, 0.12)';

/** Schematic of the q-mass bar: sorted demand, cumulative mass to q, θ at the crossing. */
export const QMassBarDiagram = () => {
  // Illustrative decreasing demand shares (not a live snapshot).
  const heights = [92, 78, 66, 58, 50, 44, 38, 34, 30, 27, 24, 22, 20, 18, 16, 14, 13, 12, 11, 10];
  const barW = 22;
  const gap = 6;
  const originX = 56;
  const baseline = 210;
  const barRank = 7; // 1-indexed visual "rank ~32" stand-in in this compressed set
  const barIndex = barRank - 1;

  const bars = heights.map((h, i) => {
    const x = originX + i * (barW + gap);
    const y = baseline - h;
    const above = i < barIndex;
    return {x, y, h, above, i};
  });

  const thetaX = originX + barIndex * (barW + gap) + barW / 2;
  const thetaY = baseline - heights[barIndex];

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 300'
      role='img'
      aria-label='Sorted subnet demand shares. Walk down the ranking until the cumulative mass reaches q; the share at that crossing is the bar theta. Subnets above the bar jointly carry q of demand.'
    >
      <text {...GRAPH_TEXT} x='380' y='28' textAnchor='middle' fill={MUTED}>
        SORTED DEMAND SHARES · WALK UNTIL CUMULATIVE ≥ q
      </text>

      {/* Above-bar mass band */}
      <rect
        x={originX - 4}
        y={40}
        width={barIndex * (barW + gap) + 4}
        height={baseline - 40}
        fill={ACCENT_SOFT}
      />
      <text
        {...GRAPH_TEXT}
        x={originX + (barIndex * (barW + gap)) / 2 - 2}
        y='58'
        textAnchor='middle'
        fill={ACCENT}
      >
        MASS ABOVE BAR ≈ q
      </text>

      {/* Axes */}
      <line x1={originX - 8} y1='40' x2={originX - 8} y2={baseline} stroke={INK} strokeWidth='1' />
      <line
        x1={originX - 8}
        y1={baseline}
        x2={originX + heights.length * (barW + gap)}
        y2={baseline}
        stroke={INK}
        strokeWidth='1'
      />
      <text {...GRAPH_TEXT} x={originX - 14} y='48' textAnchor='end'>
        SHARE
      </text>
      <text
        {...GRAPH_TEXT}
        x={originX + heights.length * (barW + gap)}
        y={baseline + 28}
        textAnchor='end'
      >
        RANK →
      </text>

      {bars.map(({x, y, h, above, i}) => (
        <rect
          key={i}
          x={x}
          y={y}
          width={barW}
          height={h}
          fill={above ? ACCENT : 'none'}
          stroke={above ? ACCENT : INK}
          strokeWidth='1.25'
        />
      ))}

      {/* Theta marker */}
      <line
        x1={thetaX}
        y1={thetaY - 8}
        x2={thetaX}
        y2={baseline + 8}
        stroke={ACCENT}
        strokeWidth='1.5'
        strokeDasharray='3 3'
      />
      <circle cx={thetaX} cy={thetaY} r='4' fill={ACCENT} />
      <text {...GRAPH_TEXT} x={thetaX} y={baseline + 44} textAnchor='middle' fill={ACCENT}>
        θ · BAR
      </text>
      <text {...GRAPH_TEXT} x={thetaX + 54} y={thetaY + 4} fill={MUTED}>
        gate = ½ here
      </text>

      <text {...GRAPH_TEXT} x={originX + 4} y={baseline + 44} fill={MUTED}>
        HEAD
      </text>
      <text
        {...GRAPH_TEXT}
        x={originX + (heights.length - 1) * (barW + gap) + barW / 2}
        y={baseline + 44}
        textAnchor='middle'
        fill={MUTED}
      >
        TAIL
      </text>
    </svg>
  );
};

/** Hill / sigmoid gate around θ — passes ½ at the bar, ~1 above, ~0 below. */
export const GateCurveDiagram = () => {
  const x0 = 70;
  const y0 = 40;
  const w = 620;
  const h = 200;
  const baseline = y0 + h;

  // Relative share s/θ from high (left) to low (right), matching rank-order
  // charts where the head sits on the left. gate = 1 / (1 + (θ/s)^h), h=3.
  const tMin = 0.15;
  const tMax = 3.0;
  const samples: Array<{t: number; g: number}> = [];
  for (let i = 0; i <= 48; i++) {
    const t = tMin + (i / 48) * (tMax - tMin);
    const ratio = 1 / t;
    const g = 1 / (1 + Math.pow(ratio, 3));
    samples.push({t, g});
  }
  const xFor = (t: number) => x0 + ((tMax - t) / (tMax - tMin)) * w;
  const yFor = (g: number) => baseline - g * h;
  const path = samples
    .map((s, i) => `${i === 0 ? 'M' : 'L'} ${xFor(s.t).toFixed(1)} ${yFor(s.g).toFixed(1)}`)
    .join(' ');
  const barX = xFor(1);

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 300'
      role='img'
      aria-label='Hill gate with exponent h equals 3. At the bar the gate passes one half of a subnet share; well above the bar the gate approaches one; deep below it approaches zero.'
    >
      <text {...GRAPH_TEXT} x='380' y='28' textAnchor='middle' fill={MUTED}>
        GATE(s) = s³ / (s³ + θ³) · h = 3
      </text>

      <line x1={x0} y1={y0} x2={x0} y2={baseline} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={baseline} x2={x0 + w} y2={baseline} stroke={INK} strokeWidth='1' />

      {/* Half-pass guide */}
      <line
        x1={x0}
        y1={yFor(0.5)}
        x2={x0 + w}
        y2={yFor(0.5)}
        stroke={FAINT}
        strokeWidth='1'
        strokeDasharray='4 3'
      />
      <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(0.5) + 3} textAnchor='end' fill={MUTED}>
        ½
      </text>
      <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(1) + 3} textAnchor='end'>
        1
      </text>
      <text {...GRAPH_TEXT} x={x0 - 8} y={baseline + 3} textAnchor='end'>
        0
      </text>

      <path d={path} fill='none' stroke={ACCENT} strokeWidth='2' />

      <line
        x1={barX}
        y1={y0}
        x2={barX}
        y2={baseline}
        stroke={ACCENT}
        strokeWidth='1.25'
        strokeDasharray='3 3'
      />
      <circle cx={barX} cy={yFor(0.5)} r='4' fill={ACCENT} />
      <text {...GRAPH_TEXT} x={barX} y={baseline + 28} textAnchor='middle' fill={ACCENT}>
        s = θ
      </text>

      <text {...GRAPH_TEXT} x={xFor(2.2)} y={yFor(0.92) - 8} textAnchor='middle' fill={MUTED}>
        ABOVE BAR ≈ 1
      </text>
      <text {...GRAPH_TEXT} x={xFor(0.35)} y={yFor(0.12) - 8} textAnchor='middle' fill={MUTED}>
        DEEP TAIL ≈ 0
      </text>

      <text {...GRAPH_TEXT} x={x0} y={baseline + 28}>
        HEAD
      </text>
      <text {...GRAPH_TEXT} x={x0 + w} y={baseline + 28} textAnchor='end'>
        TAIL →
      </text>
      <text {...GRAPH_TEXT} x={x0 - 36} y={y0 + 12} textAnchor='middle'>
        GATE
      </text>
    </svg>
  );
};

/** Before/after: idle slot carry props registration cost; after the gate, entry falls toward tx fee. */
export const SlotCostDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 300'
    role='img'
    aria-label='Before the gate, an idle slot still earned a slice of every block, so registration cost sat near 1300 TAO. After the gate, idle emission collapses and the price of entry should fall toward the registration transaction fee.'
  >
    <text {...GRAPH_TEXT} x='190' y='32' textAnchor='middle' fill={MUTED}>
      BEFORE · FLAT PRICE EMISSION
    </text>
    <text {...GRAPH_TEXT} x='570' y='32' textAnchor='middle' fill={MUTED}>
      AFTER · EMISSION GATE
    </text>

    {/* Before panel */}
    <rect x='40' y='48' width='300' height='220' fill='none' stroke={FAINT} strokeWidth='1' />
    <rect x='100' y='78' width='72' height='140' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='136' y='160' textAnchor='middle'>
      IDLE
    </text>
    <text {...GRAPH_TEXT} x='136' y='176' textAnchor='middle'>
      YIELD
    </text>
    <text {...GRAPH_TEXT} x='136' y='234' textAnchor='middle' fill={MUTED}>
      STILL PAID
    </text>

    <rect
      x='208'
      y='68'
      width='88'
      height='150'
      fill={ACCENT_SOFT}
      stroke={ACCENT}
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='252' y='130' textAnchor='middle' fill={ACCENT}>
      ~1300
    </text>
    <text {...GRAPH_TEXT} x='252' y='148' textAnchor='middle' fill={ACCENT}>
      TAO
    </text>
    <text {...GRAPH_TEXT} x='252' y='234' textAnchor='middle' fill={MUTED}>
      ENTRY COST
    </text>

    <text {...GRAPH_TEXT} x='190' y='255' textAnchor='middle' fill={MUTED}>
      PASSIVE CARRY PROPS THE SLOT
    </text>

    {/* After panel */}
    <rect
      x='420'
      y='48'
      width='300'
      height='220'
      fill='rgba(209, 81, 104, 0.04)'
      stroke={ACCENT}
      strokeWidth='1'
    />
    <rect x='480' y='168' width='72' height='50' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='516' y='192' textAnchor='middle'>
      IDLE
    </text>
    <text {...GRAPH_TEXT} x='516' y='208' textAnchor='middle'>
      ≈ 0
    </text>
    <text {...GRAPH_TEXT} x='516' y='234' textAnchor='middle' fill={MUTED}>
      DRIP ONLY
    </text>

    <rect x='588' y='188' width='88' height='30' fill='none' stroke={ACCENT} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='632' y='208' textAnchor='middle' fill={ACCENT}>
      ~TX FEE
    </text>
    <text {...GRAPH_TEXT} x='632' y='234' textAnchor='middle' fill={MUTED}>
      ENTRY COST
    </text>

    <text {...GRAPH_TEXT} x='570' y='255' textAnchor='middle' fill={ACCENT}>
      A SLOT IS A STARTING POSITION
    </text>
  </svg>
);
