/**
 * Shared chart theme for the docs explainer figures.
 *
 * Design rules (the "research-paper figure" language):
 * - Monochrome ink on white. INK for primary series/text, INK_FAINT for
 *   secondary series, axis ticks, and axis titles.
 * - Red (ACCENT) is reserved for thresholds, warnings, and highlight points
 *   only — never for a regular data series. Tint warning regions with
 *   ACCENT_REGION; wash accented stat values with ACCENT_WASH.
 * - No Chart.js legends. Label series directly in-plot with uppercase
 *   FiraCode annotations (GRAPH_FONT) drawn via a Chart.js plugin.
 * - Keep axes quiet: grid GRID, border AXIS_BORDER, ticks via baseTicks()
 *   with maxTicksLimit 5–6, axis titles via axisTitle().
 */

export const INK = 'rgb(41, 41, 41)';
export const INK_FAINT = 'rgba(41, 41, 41, 0.45)';
export const GRID = 'rgba(41, 41, 41, 0.05)';
export const AXIS_BORDER = 'rgba(41, 41, 41, 0.25)';
export const ACCENT = '#d15168';
export const ACCENT_REGION = 'rgba(209, 81, 104, 0.05)';
export const ACCENT_WASH = 'rgba(209, 81, 104, 0.08)';

/** Canvas ctx.font string for in-plot annotations drawn by plugins. */
export const GRAPH_FONT = '10px FiraCode, monospace';

/** Base tick styling for any scale; spread first, then override. */
export function baseTicks(overrides?: Record<string, unknown>) {
  return {
    maxTicksLimit: 6,
    font: { family: 'FiraCode, monospace', size: 10 },
    color: INK_FAINT,
    ...overrides,
  };
}

/** Muted 10px axis title. */
export function axisTitle(text: string) {
  return {
    display: true,
    text,
    font: { size: 10 },
    color: INK_FAINT,
  };
}
