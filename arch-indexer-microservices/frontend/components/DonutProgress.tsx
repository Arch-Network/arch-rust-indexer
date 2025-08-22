import React from 'react';

type Props = {
  size?: number;
  thickness?: number;
  percent: number; // 0-100
  label?: string;
  sublabel?: string;
};

export default function DonutProgress({ size = 160, thickness = 12, percent, label = 'Sync', sublabel }: Props) {
  const r = (size - thickness) / 2;
  const c = 2 * Math.PI * r;
  const clamped = Math.max(0, Math.min(100, percent || 0));
  const dash = (clamped / 100) * c;
  const rest = c - dash;

  return (
    <div style={{ display: 'inline-flex', alignItems: 'center', gap: 16 }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <g transform={`rotate(-90 ${size/2} ${size/2})`}>
          <circle cx={size/2} cy={size/2} r={r} fill="none" stroke="#0a0c10" strokeWidth={thickness} />
          <circle
            cx={size/2}
            cy={size/2}
            r={r}
            fill="none"
            stroke="url(#grad)"
            strokeWidth={thickness}
            strokeDasharray={`${dash} ${rest}`}
            strokeLinecap="round"
          />
          <defs>
            <linearGradient id="grad" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="var(--accent)" />
              <stop offset="100%" stopColor="var(--accent-2)" />
            </linearGradient>
          </defs>
        </g>
        <text x="50%" y="46%" dominantBaseline="middle" textAnchor="middle" fill="var(--text)" fontWeight={700} fontSize={size * 0.22}> {Math.round(clamped)}% </text>
        <text x="50%" y="62%" dominantBaseline="middle" textAnchor="middle" fill="var(--muted)" fontSize={size * 0.12}> {label} </text>
      </svg>
      {sublabel && (
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <div style={{ color: 'var(--muted)', fontSize: 12, textTransform: 'uppercase', letterSpacing: '.08em' }}>Status</div>
          <div style={{ color: 'var(--text)', fontWeight: 700 }}>{sublabel}</div>
        </div>
      )}
    </div>
  );
}
