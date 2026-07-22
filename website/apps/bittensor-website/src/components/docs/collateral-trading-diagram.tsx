import {ACCENT, ACCENT_REGION, INK, INK_FAINT} from './chart-theme';

const T = {
  fontFamily: 'FiraCode, monospace',
  fontSize: 10,
  fill: INK,
} as const;

/**
 * Trading / Sortino subnet: a martingale farmer looks profitable under pure
 * burn, but a high lock share + slow drain strands the bond when validators
 * stop scoring after the blow-up.
 */
export function TradingCollateralDiagram({className}: {className?: string}) {
  return (
    <svg
      className={className}
      viewBox='0 0 760 360'
      role='img'
      aria-label='Trading-signals subnet: under pure burn a martingale farmer recycles cheaply after blow-up; with a 90 percent lock and slow drain the bond strands when validators stop scoring.'
    >
      <rect x='20' y='28' width='350' height='300' fill='none' stroke={INK} strokeWidth='1' />
      <rect x='390' y='28' width='350' height='300' fill='none' stroke={INK} strokeWidth='1' />

      <text {...T} x='195' y='48' textAnchor='middle'>
        PURE BURN · PRICE τ10
      </text>
      <text {...T} x='565' y='48' textAnchor='middle'>
        COLLATERAL · p=90% · k=0.2
      </text>

      {[
        {y: 78, label: '1. REGISTER', detail: 'PAY τ10 BURNED'},
        {y: 128, label: '2. POST HIGH SORTINO', detail: 'SELLING UNSEEN TAIL RISK'},
        {y: 178, label: '3. FARM ~τ80 EMISSIONS', detail: 'METRIC LOOKS GREAT'},
        {y: 228, label: '4. BLOW UP · NEW HOTKEY', detail: 'NET ~τ70 · REPEAT'},
      ].map((step) => (
        <g key={step.label}>
          <text {...T} x='40' y={step.y}>
            {step.label}
          </text>
          <text {...T} x='40' y={step.y + 14} fill={INK_FAINT}>
            {step.detail}
          </text>
        </g>
      ))}
      <text {...T} x='195' y='290' textAnchor='middle' fill={ACCENT}>
        FORGETTING COSTS ONLY THE NEXT BURN
      </text>
      <text {...T} x='195' y='306' textAnchor='middle' fill={INK_FAINT}>
        SORTINO NEVER SAW THE TAIL
      </text>

      <rect x='410' y='68' width='310' height='52' fill={ACCENT_REGION} />
      <text {...T} x='425' y='88'>
        1. REGISTER · SAME τ10
      </text>
      <text {...T} x='425' y='104' fill={INK_FAINT}>
        τ1 BURNED · τ9 LOCKED AS α
      </text>

      <text {...T} x='425' y='148'>
        2–3. FARM WHILE LOOKING GOOD
      </text>
      <text {...T} x='425' y='164' fill={INK_FAINT}>
        SLOW DRAIN · MOST OF τ9 STILL LOCKED
      </text>

      <line
        x1='425'
        y1='188'
        x2='700'
        y2='188'
        stroke={ACCENT}
        strokeWidth='1'
        strokeDasharray='3 3'
      />
      <text {...T} x='425' y='208' fill={ACCENT}>
        4. BLOW UP · VALIDATORS STOP SCORING
      </text>
      <text {...T} x='425' y='224' fill={ACCENT}>
        INCENTIVE → 0 · DRAIN FREEZES
      </text>

      <rect x='410' y='244' width='310' height='52' fill={ACCENT_REGION} />
      <text {...T} x='565' y='266' textAnchor='middle' fill={ACCENT}>
        REMAINING BOND STRANDS
      </text>
      <text {...T} x='565' y='282' textAnchor='middle' fill={INK_FAINT}>
        E* ≈ T ÷ 1.2 BEFORE DETECTION JUST TO BREAK EVEN
      </text>
    </svg>
  );
}
