"use strict";

const DISCORD_CONTENT_LIMIT = 2000;
const DISCORD_EMBED_DESCRIPTION_LIMIT = 4096;
const TRUNCATION_NOTE = "... [decoded call truncated; see run URL]";

function compactValue(value) {
  if (value && typeof value === "object" && !Array.isArray(value) &&
      typeof value.section === "string" && typeof value.method === "string") {
    return `${value.section}.${value.method}(${compactValue(value.args || {})})`;
  }
  if (Array.isArray(value)) return `[${value.map(compactValue).join(", ")}]`;
  if (value && typeof value === "object") {
    return `{${Object.entries(value).map(([key, item]) => `${key}: ${compactValue(item)}`).join(", ")}}`;
  }
  return JSON.stringify(value);
}

function truncateDecoded(decoded, limit) {
  if (decoded.length <= limit) return { text: decoded, truncated: false };
  if (limit <= TRUNCATION_NOTE.length) return { text: TRUNCATION_NOTE, truncated: true };
  return { text: decoded.slice(0, limit - TRUNCATION_NOTE.length) + TRUNCATION_NOTE, truncated: true };
}

function renderPayload(proposal, context) {
  const proposed = proposal.diff?.proposed;
  if (!Array.isArray(proposed) || !proposed.length || !proposal.call_hash || !proposal.decoded_call) {
    throw new Error("Discord payloads require a real encoded proposal with at least one change");
  }
  const changes = proposed.map(({ key, live, proposed: desired }) =>
    `${key}: ${JSON.stringify(live)} -> ${JSON.stringify(desired)}`,
  ).join("\n");
  const decoded = compactValue(proposal.decoded_call);
  const source = context.prUrl ? `Merged PR: <${context.prUrl}>` : `Source commit: <${context.commitUrl}>`;
  const suffix = [
    `Call hash: \`${proposal.call_hash}\``,
    source,
    `Run / countersign command: <${context.runUrl}>`,
    `Timestamp (UTC): ${context.timestamp}`,
  ].join("\n");
  const header = `**Sudo parameter proposal: ${proposal.network}**`;
  const prefix = `${header}\n\`\`\`text\n${changes}\n\`\`\`\nDecoded call:\n\`\`\`text\n`;
  const ending = "\n```\n" + suffix;
  const shortened = truncateDecoded(decoded, DISCORD_CONTENT_LIMIT - prefix.length - ending.length);
  const payload = { content: prefix + shortened.text + ending, allowed_mentions: { parse: [] } };
  if (payload.content.length <= DISCORD_CONTENT_LIMIT) return payload;

  // Long change lists can leave no room for a call in content. Keep every
  // change in an embed instead of dropping or splitting a network's proposal.
  const embedPrefix = `${changes}\n\nDecoded call:\n\`\`\`text\n`;
  const embedEnding = "\n```";
  const embedded = truncateDecoded(decoded, DISCORD_EMBED_DESCRIPTION_LIMIT - embedPrefix.length - embedEnding.length);
  const description = embedPrefix + embedded.text + embedEnding;
  if (description.length > DISCORD_EMBED_DESCRIPTION_LIMIT) {
    throw new Error("Proposed changes exceed Discord's single-message embed limit; inspect the run URL");
  }
  return {
    content: header,
    embeds: [{
      description,
      fields: [
        { name: "Call hash", value: proposal.call_hash },
        { name: context.prUrl ? "Merged PR" : "Source commit", value: `[Open](${context.prUrl || context.commitUrl})` },
        { name: "Run / countersign command", value: `[Open workflow summary](${context.runUrl})` },
        { name: "Timestamp (UTC)", value: context.timestamp },
      ],
    }],
    allowed_mentions: { parse: [] },
  };
}

function trustedContext(env) {
  return (env.GITHUB_EVENT_NAME === "push" || env.GITHUB_EVENT_NAME === "workflow_dispatch") &&
    env.GITHUB_REF === "refs/heads/main";
}

async function resolveMergedPr(env, fetchImpl, log) {
  const server = env.GITHUB_SERVER_URL || "https://github.com";
  const commitUrl = `${server}/${env.GITHUB_REPOSITORY}/commit/${env.GITHUB_SHA}`;
  if (!env.GITHUB_TOKEN) {
    log("::warning::GITHUB_TOKEN is unavailable; using the commit link in the Discord notification.");
    return { commitUrl, prUrl: null };
  }
  const api = env.GITHUB_API_URL || "https://api.github.com";
  try {
    const response = await fetchImpl(`${api}/repos/${env.GITHUB_REPOSITORY}/commits/${env.GITHUB_SHA}/pulls`, {
      headers: {
        accept: "application/vnd.github+json",
        authorization: `Bearer ${env.GITHUB_TOKEN}`,
        "user-agent": "subtensor-sudo-proposal-notifier",
        "x-github-api-version": "2022-11-28",
      },
    });
    if (!response.ok) {
      log(`::warning::Merged PR lookup returned HTTP ${response.status}; using the commit link.`);
      return { commitUrl, prUrl: null };
    }
    const pulls = await response.json();
    const merged = Array.isArray(pulls) && pulls.find((pull) => pull.merged_at && pull.html_url);
    return { commitUrl, prUrl: merged ? merged.html_url : null };
  } catch {
    log("::warning::Merged PR lookup failed; using the commit link in the Discord notification.");
    return { commitUrl, prUrl: null };
  }
}

async function notifyProposal({ proposal, env, fetchImpl = fetch, now = () => new Date(), log = console.log }) {
  const network = proposal.network;
  if (!trustedContext(env)) {
    log(`::notice::Discord notification for ${network} skipped: notifications are limited to push and workflow_dispatch on main.`);
    return { status: "skipped-untrusted" };
  }
  const webhook = env.SUDO_DISCORD_WEBHOOK?.trim();
  if (!webhook) {
    log(`::warning::SUDO_DISCORD_WEBHOOK is unset or empty; Discord notification for ${network} skipped.`);
    return { status: "skipped-no-webhook" };
  }
  if (!Array.isArray(proposal.diff?.proposed) || !proposal.diff.proposed.length ||
      !proposal.call_data || !proposal.call_hash || !proposal.decoded_call) {
    log(`::notice::Discord notification for ${network} skipped: no encoded proposed changes.`);
    return { status: "skipped-no-changes" };
  }
  const server = env.GITHUB_SERVER_URL || "https://github.com";
  const runUrl = `${server}/${env.GITHUB_REPOSITORY}/actions/runs/${env.GITHUB_RUN_ID}`;
  const timestamp = now().toISOString();
  const { commitUrl, prUrl } = await resolveMergedPr(env, fetchImpl, log);
  const payload = renderPayload(proposal, { commitUrl, prUrl, runUrl, timestamp });
  let response;
  try {
    response = await fetchImpl(webhook, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
  } catch {
    throw new Error(`Discord webhook request failed for ${network}; the webhook URL was not logged.`);
  }
  if (!response.ok) throw new Error(`Discord webhook returned HTTP ${response.status} for ${network}; the webhook URL was not logged.`);
  log(`::notice::Discord notification posted for ${network} (${proposal.diff.proposed.length} proposed value change(s)).`);
  return { status: "posted", payload, timestamp };
}

module.exports = { compactValue, notifyProposal, renderPayload, resolveMergedPr, trustedContext };
