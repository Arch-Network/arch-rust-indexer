import React, { useMemo } from 'react';

type Props = {
  data: number[];
  width?: number;
  height?: number;
  color?: string;
  fillOpacity?: number;
};

export default function Sparkline({ data, width = 160, height = 40, color = 'var(--accent)', fillOpacity = 0.12 }: Props) {
  const { path, area } = useMemo(() => {
    if (!data || data.length === 0) return { path: '', area: '' };
    const max = Math.max(1, ...data);
    const min = Math.min(0, ...data);
    const range = Math.max(1e-9, max - min);
    const step = data.length > 1 ? width / (data.length - 1) : width;

    const points = data.map((v, i) => {
      const x = i * step;
      const y = height - ((v - min) / range) * height;
      return `${x},${y}`;
    });

    const d = `M ${points.join(' L ')}`;
    const a = `M 0,${height} L ${points.join(' L ')} L ${width},${height} Z`;
    return { path: d, area: a };
  }, [data, width, height]);

  return (
    <svg width={width} height={height} style={{ display: 'block' }}>
      <path d={area} fill={color} opacity={fillOpacity} />
      <path d={path} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
