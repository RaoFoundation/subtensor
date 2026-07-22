import {ACCENT, ACCENT_REGION, INK, INK_FAINT} from './chart-theme';

const T = {
  fontFamily: 'FiraCode, monospace',
  fontSize: 10,
  fill: INK,
} as const;

/**
 * Lium-style GPU marketplace: per-machine deposits via add_collateral +
 * set_min_collateral floor, with a clean wind-down versus a pulled rental
 * that strands the deposit.
 */
export function LiumCollateralDiagram({className}: {className?: string}) {
  return (
    <svg
      className={className}
      viewBox='0 0 760 380'
      role='img'
      aria-label='Lium-style GPU marketplace: four machines need 100 alpha collateral, held by a floor; honest wind-down drains the deposit while pulling a rented machine strands it.'
    >
      <text {...T} x='380' y='28' textAnchor='middle'>
        LIUM PATTERN · 25α DEPOSIT PER MACHINE
      </text>

      {[0, 1, 2, 3].map((i) => {
        const x = 70 + i * 80;
        return (
          <g key={i}>
            <rect x={x} y='48' width='56' height='40' fill='none' stroke={INK} strokeWidth='1.5' />
            <text {...T} x={x + 28} y='72' textAnchor='middle'>
              GPU {i + 1}
            </text>
          </g>
        );
      })}
      <text {...T} x='430' y='64'>
        × 25α =
      </text>
      <rect
        x='500'
        y='48'
        width='180'
        height='40'
        fill={ACCENT_REGION}
        stroke={ACCENT}
        strokeWidth='1.5'
      />
      <text {...T} x='590' y='72' textAnchor='middle' fill={ACCENT}>
        100α REQUIRED
      </text>

      <line x1='70' y1='110' x2='690' y2='110' stroke='rgba(41, 41, 41, 0.2)' strokeWidth='1' />
      <text {...T} x='70' y='132'>
        MINER
      </text>
      <text {...T} x='70' y='152' fill={INK_FAINT}>
        btcli collateral add --amount-tao 100
      </text>
      <text {...T} x='70' y='166' fill={INK_FAINT}>
        btcli collateral set-min --min-alpha 100
      </text>

      <text {...T} x='420' y='132'>
        LOCK ON HOTKEY
      </text>
      <rect x='420' y='142' width='270' height='28' fill='none' stroke={INK} strokeWidth='1.5' />
      <rect x='420' y='142' width='270' height='28' fill='rgba(41, 41, 41, 0.06)' />
      <line x1='420' y1='142' x2='690' y2='142' stroke={ACCENT} strokeWidth='2' />
      <text {...T} x='555' y='161' textAnchor='middle'>
        100α LOCKED · FLOOR HOLDS
      </text>
      <text {...T} x='555' y='188' textAnchor='middle' fill={INK_FAINT}>
        DRAIN NEVER RELEASES BELOW set_min_collateral
      </text>

      <line x1='70' y1='210' x2='690' y2='210' stroke='rgba(41, 41, 41, 0.2)' strokeWidth='1' />

      <rect x='40' y='228' width='330' height='126' fill='none' stroke={INK} strokeWidth='1' />
      <text {...T} x='205' y='250' textAnchor='middle'>
        HONEST EXIT · WIND-DOWN
      </text>
      <text {...T} x='55' y='274' fill={INK_FAINT}>
        CLEAR FLOOR · KEEP SERVING RENTALS
      </text>
      <text {...T} x='55' y='292' fill={INK_FAINT}>
        VALIDATORS KEEP SCORING
      </text>
      <text {...T} x='55' y='310' fill={INK_FAINT}>
        DRAIN RETURNS DEPOSIT AT k × INCENTIVE
      </text>
      <text {...T} x='205' y='338' textAnchor='middle'>
        CAPITAL COMES BACK WITH THE WORK
      </text>

      <rect x='390' y='228' width='330' height='126' fill='none' stroke={ACCENT} strokeWidth='1.5' />
      <rect x='390' y='228' width='330' height='126' fill={ACCENT_REGION} />
      <text {...T} x='555' y='250' textAnchor='middle' fill={ACCENT}>
        PULL A RENTED MACHINE
      </text>
      <text {...T} x='405' y='274' fill={INK_FAINT}>
        SCORE → 0 IMMEDIATELY
      </text>
      <text {...T} x='405' y='292' fill={INK_FAINT}>
        DRAIN STOPS · DEPOSIT STRANDS
      </text>
      <text {...T} x='405' y='310' fill={INK_FAINT}>
        HOTKEY HISTORY PERSISTS
      </text>
      <text {...T} x='555' y='338' textAnchor='middle' fill={ACCENT}>
        FORFEIT UNTIL YOU MINE IT OUT
      </text>
    </svg>
  );
}
