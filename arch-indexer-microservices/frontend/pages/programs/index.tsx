import React, { useEffect, useMemo, useState } from 'react';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';
import Pagination from '../../components/Pagination';

export default function ProgramsPage() {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [term, setTerm] = useState('');
  const [programs, setPrograms] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const pageSize = 100;

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
        const params = new URLSearchParams({ limit: String(pageSize), page: String(page) });
        const q = term.trim();
        if (q) params.set('search', q);
        const res = await fetch(`${apiUrl}/api/programs?${params.toString()}`);
        const json = await res.json();
        setPrograms(json.programs || []);
        setTotal(json.total_count || 0);
      } catch (e: any) {
        setError('Failed to load programs');
      } finally {
        setLoading(false);
      }
    })();
  }, [apiUrl, page, term]);

  const filtered = useMemo(() => programs, [programs]);

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
                <th>Name</th>
                <th>Program ID (hex)</th>
                <th>Program ID (base58)</th>
                <th>Transactions</th>
                <th>First Seen</th>
                <th>Last Seen</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((p: any) => (
                <tr key={p.program_id_hex || p.program_id}>
                  <td>{p.display_name || '—'}</td>
                  <td style={{ wordBreak: 'break-all' }}>
                    <a className={styles.hashButton} href={`/programs/${p.program_id_hex || p.program_id}`}>{p.program_id_hex || p.program_id}</a>
                  </td>
                  <td style={{ wordBreak: 'break-all' }}>{p.program_id_base58 || ''}</td>
                  <td>{p.transaction_count}</td>
                  <td>{p.first_seen_at ? new Date(p.first_seen_at).toLocaleString() : '—'}</td>
                  <td>{p.last_seen_at ? new Date(p.last_seen_at).toLocaleString() : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {!loading && !error && (
          <Pagination page={page} pageSize={pageSize} total={total} onPageChange={setPage} />
        )}
      </section>
    </Layout>
  );
}
