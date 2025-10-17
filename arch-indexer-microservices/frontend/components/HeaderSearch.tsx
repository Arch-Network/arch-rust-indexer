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
      const res = await fetch(`${apiUrl}/api/search?q=${encodeURIComponent(q)}`);
      const data = await res.json();
      // If server returns a bestGuess redirect, use it
      if (data?.bestGuess?.redirect && data?.bestGuess?.url) {
        router.push(data.bestGuess.url);
        return;
      }
      // Otherwise open search results page grouped by type
      router.push(`/search?q=${encodeURIComponent(q)}`);
    } catch {
      alert('Not found');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div style={{ display: 'flex', gap: 8, width: '100%', flexWrap: 'wrap' }}>
      <input
        type="text"
        className={styles.searchInput}
        placeholder="Search blocks, txs, accounts, programs, tokens…"
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
