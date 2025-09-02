import React, { useEffect, useState } from 'react';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

export default function TokensPage() {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || '';
  const [items, setItems] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(1);
  const [authority, setAuthority] = useState('');
  const pageSize = 50;

  useEffect(() => {
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const params = new URLSearchParams({ limit: String(pageSize), page: String(page) });
        const a = authority.trim();
        if (a) params.set('authority', a);
        const res = await fetch(`${apiUrl}/api/tokens/leaderboard?${params.toString()}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        setItems(json.tokens || []);
      } catch (e: any) {
        setError('Failed to load tokens');
      } finally {
        setLoading(false);
      }
    })();
  }, [apiUrl, page, authority]);

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Tokens</h2>
        <div className={styles.searchContainer}>
          <input
            className={styles.searchInput}
            placeholder="Filter by mint authority (pubkey)…"
            value={authority}
            onChange={(e) => setAuthority(e.target.value)}
          />
        </div>
      </section>
      <section className={styles.searchSection}>
        <h2>Leaderboard</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {!loading && !error && (
          <table className={styles.transactionsTable}>
            <thead>
              <tr>
                <th>Mint</th>
                <th>Program</th>
                <th>Holders</th>
                <th>Total Balance (raw)</th>
                <th>Decimals</th>
                <th>Supply</th>
                <th>Mint Authority</th>
              </tr>
            </thead>
            <tbody>
              {items.map((t: any) => (
                <tr key={t.mint_address}>
                  <td style={{ wordBreak: 'break-all', fontFamily: 'monospace' }}>{t.mint_address}</td>
                  <td style={{ wordBreak: 'break-all', fontFamily: 'monospace' }}>{t.program_id}</td>
                  <td>{t.holders?.toLocaleString?.() ?? t.holders}</td>
                  <td>{t.total_balance}</td>
                  <td>{t.decimals}</td>
                  <td>{t.supply ?? '—'}</td>
                  <td style={{ wordBreak: 'break-all', fontFamily: 'monospace' }}>{t.mint_authority ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
          <button className={styles.searchButton} onClick={() => setPage(Math.max(1, page - 1))} disabled={page === 1}>Prev</button>
          <button className={styles.searchButton} onClick={() => setPage(page + 1)}>Next</button>
          <span style={{ opacity: 0.7 }}>Page {page}</span>
        </div>
      </section>
    </Layout>
  );
}
