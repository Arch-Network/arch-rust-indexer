import React, { useEffect, useMemo, useState } from 'react';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

export default function ProgramsPage() {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [term, setTerm] = useState('');
  const [programs, setPrograms] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const go = () => {
    const pid = term.trim();
    if (!pid) return;
    window.location.href = `/programs/${encodeURIComponent(pid)}`;
  };

  useEffect(() => {
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/programs/leaderboard`);
        const json = await res.json();
        setPrograms(Array.isArray(json) ? json : (json.items || []));
      } catch (e: any) {
        setError('Failed to load programs');
      } finally {
        setLoading(false);
      }
    })();
  }, [apiUrl]);

  const filtered = useMemo(() => {
    const q = term.trim().toLowerCase();
    if (!q) return programs;
    return programs.filter((p: any) => (p.program_id || '').toLowerCase().includes(q));
  }, [programs, term]);

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Programs</h2>
        <div className={styles.searchContainer}>
          <input
            className={styles.searchInput}
            placeholder="Enter Program ID (pubkey)…"
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && go()}
          />
          <button className={styles.searchButton} onClick={go}>Open</button>
        </div>
      </section>
      <section className={styles.searchSection}>
        <h2>All Programs</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {!loading && !error && (
          <table className={styles.transactionsTable}>
            <thead>
              <tr>
                <th>Program ID</th>
                <th>Transactions</th>
                <th>First Seen</th>
                <th>Last Seen</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((p: any) => (
                <tr key={p.program_id}>
                  <td style={{ wordBreak: 'break-all' }}>
                    <a className={styles.hashButton} href={`/programs/${p.program_id}`}>{p.program_id}</a>
                  </td>
                  <td>{p.transaction_count}</td>
                  <td>{p.first_seen_at ? new Date(p.first_seen_at).toLocaleString() : '—'}</td>
                  <td>{p.last_seen_at ? new Date(p.last_seen_at).toLocaleString() : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </Layout>
  );
}
