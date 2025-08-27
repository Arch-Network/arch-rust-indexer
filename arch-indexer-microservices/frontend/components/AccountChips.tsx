import React, { useMemo, useState } from 'react';
import Link from 'next/link';
import styles from './AccountChips.module.css';

type AccountChipsProps = {
  accounts: string[];
  maxVisible?: number;
};

function middleTruncate(value: string, max: number): string {
  if (value.length <= max) return value;
  const half = Math.max(1, Math.floor((max - 1) / 2));
  return `${value.slice(0, half)}…${value.slice(-half)}`;
}

const AccountChips: React.FC<AccountChipsProps> = ({ accounts, maxVisible = 6 }) => {
  const [expanded, setExpanded] = useState(false);

  const [visible, hidden] = useMemo(() => {
    if (expanded) return [accounts, [] as string[]];
    return [accounts.slice(0, maxVisible), accounts.slice(maxVisible)];
  }, [accounts, expanded, maxVisible]);

  if (!accounts || accounts.length === 0) {
    return <span className={styles.muted}>No accounts</span>;
  }

  return (
    <div className={styles.wrapper}>
      {visible.map((a, i) => (
        <span key={`${a}-${i}`} className={styles.chip} title={a}>
          <Link href={`/accounts/${a}`} className={styles.chipLink}>
            {middleTruncate(a, 18)}
          </Link>
          <button
            className={styles.copyBtn}
            onClick={async (e) => {
              e.preventDefault();
              e.stopPropagation();
              try { await navigator.clipboard.writeText(a); } catch {}
            }}
            aria-label="Copy account"
            title="Copy"
          >
            ⎘
          </button>
        </span>
      ))}
      {hidden.length > 0 && (
        <button className={styles.moreBtn} onClick={() => setExpanded(v => !v)}>
          {expanded ? 'Show less' : `Show ${hidden.length} more`}
        </button>
      )}
    </div>
  );
};

export default AccountChips;
