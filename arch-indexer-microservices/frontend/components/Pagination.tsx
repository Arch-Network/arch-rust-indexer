import React from 'react';
import Button from './Button';

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
      <Button size="sm" variant="secondary" disabled={!canPrev} onClick={() => goto(1)}>« First</Button>
      <Button size="sm" variant="secondary" disabled={!canPrev} onClick={() => goto(clampedPage - 1)}>‹ Prev</Button>
      <div style={{ display: 'flex', gap: 6 }}>
        {pages.map((p) => (
          <Button
            key={p}
            size="sm"
            variant={p === clampedPage ? 'primary' : 'secondary'}
            style={p === clampedPage ? { borderColor: 'var(--accent)', color: 'var(--accent)', fontWeight: 700 } : undefined}
            onClick={() => goto(p)}
          >
            {p}
          </Button>
        ))}
      </div>
      <Button size="sm" variant="secondary" disabled={!canNext} onClick={() => goto(clampedPage + 1)}>Next ›</Button>
      <Button size="sm" variant="secondary" disabled={!canNext} onClick={() => goto(totalPages)}>Last »</Button>
      <span style={{ marginLeft: 8, color: 'var(--muted)', fontSize: 12 }}>
        Page {clampedPage} / {totalPages} · {total.toLocaleString()} items
      </span>
    </div>
  );
}

// deprecated local button styles replaced by shared Button component
