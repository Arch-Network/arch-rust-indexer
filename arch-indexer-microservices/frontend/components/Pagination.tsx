import React from 'react';

type Props = {
  page: number; // 1-indexed
  pageSize: number;
  total: number;
  onPageChange: (page: number) => void;
};

export default function Pagination({ page, pageSize, total, onPageChange }: Props) {
  const totalPages = Math.max(1, Math.ceil((total || 0) / (pageSize || 1)));
  const clampedPage = Math.min(Math.max(1, page), totalPages);

  const canPrev = clampedPage > 1;
  const canNext = clampedPage < totalPages;

  const goto = (p: number) => {
    const next = Math.min(Math.max(1, p), totalPages);
    if (next !== clampedPage) onPageChange(next);
  };

  const windowSize = 5;
  const start = Math.max(1, clampedPage - Math.floor(windowSize / 2));
  const end = Math.min(totalPages, start + windowSize - 1);
  const pages = [] as number[];
  for (let i = start; i <= end; i++) pages.push(i);

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 12 }}>
      <button disabled={!canPrev} onClick={() => goto(1)} style={btn}>« First</button>
      <button disabled={!canPrev} onClick={() => goto(clampedPage - 1)} style={btn}>‹ Prev</button>
      <div style={{ display: 'flex', gap: 6 }}>
        {pages.map((p) => (
          <button
            key={p}
            onClick={() => goto(p)}
            style={p === clampedPage ? { ...btn, ...btnActive } : btn}
          >
            {p}
          </button>
        ))}
      </div>
      <button disabled={!canNext} onClick={() => goto(clampedPage + 1)} style={btn}>Next ›</button>
      <button disabled={!canNext} onClick={() => goto(totalPages)} style={btn}>Last »</button>
      <span style={{ marginLeft: 8, color: 'var(--muted)', fontSize: 12 }}>
        Page {clampedPage} / {totalPages} · {total.toLocaleString()} items
      </span>
    </div>
  );
}

const btn: React.CSSProperties = {
  background: 'var(--panel)',
  color: 'var(--text)',
  border: '1px solid rgba(255,255,255,0.12)',
  padding: '6px 10px',
  fontSize: 12,
  cursor: 'pointer',
};

const btnActive: React.CSSProperties = {
  borderColor: 'var(--accent)',
  color: 'var(--accent)',
  fontWeight: 700,
};
