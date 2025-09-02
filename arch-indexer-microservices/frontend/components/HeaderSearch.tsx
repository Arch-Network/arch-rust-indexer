import React, { useState } from 'react';
import { useRouter } from 'next/router';
import styles from '../styles/Home.module.css';

export default function HeaderSearch() {
  const [term, setTerm] = useState('');
  const [loading, setLoading] = useState(false);
  const router = useRouter();
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || '';

  const go = async () => {
    const q = term.trim();
    if (!q) return;
    try {
      setLoading(true);
      // Fast-path: numeric => block height
      if (/^\d+$/.test(q)) {
        router.push(`/blocks/${q}`);
        return;
      }

      const res = await fetch(`${apiUrl}/api/search?term=${encodeURIComponent(q)}`);
      const json = await res.json();
      if (json?.type === 'block' && json?.data?.height != null) {
        router.push(`/blocks/${json.data.height}`);
        return;
      }
      if (json?.type === 'transaction' && json?.data?.txid) {
        router.push(`/tx/${json.data.txid}`);
        return;
      }
      // Fallback: assume block hash
      router.push(`/blocks/${q}`);
    } catch {
      alert('Not found');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div style={{ display: 'flex', gap: 8, minWidth: 420 }}>
      <input
        type="text"
        className={styles.searchInput}
        placeholder="Search height, block hash, txid, program id…"
        value={term}
        onChange={(e) => setTerm(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && go()}
      />
      <button className={`${styles.searchButton} ${styles.searchButtonSm}`} onClick={go} disabled={loading}>
        {loading ? '…' : 'Search'}
      </button>
    </div>
  );
}
