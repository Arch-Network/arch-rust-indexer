import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/router';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';
import { middleEllipsis } from '../../utils/format';

type Program = { program_id: string; transaction_count: number; first_seen_at?: string; last_seen_at?: string };

export default function ProgramDetailPage() {
  const router = useRouter();
  const id = router.query.id as string | undefined;
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [program, setProgram] = useState<Program | null>(null);
  const [recent, setRecent] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/programs/${encodeURIComponent(id)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        const p = json.program || json;
        setProgram({
          program_id: p.program_id_hex || p.program_id,
          transaction_count: p.transaction_count,
          first_seen_at: p.first_seen_at,
          last_seen_at: p.last_seen_at,
        });
        setRecent(json.recent_transactions || []);
      } catch (e: any) {
        setError('Program not found');
      } finally {
        setLoading(false);
      }
    })();
  }, [id, apiUrl]);

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Program Detail</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {program && (
          <div className={styles.statsGrid}>
            <div className={styles.statCard}>
              <h3>Program ID</h3>
              <div className={styles.value} style={{ fontSize: '1rem', wordBreak: 'break-all' }}>{program.program_id}</div>
              <div className={styles.label}>Identifier</div>
            </div>
            <div className={styles.statCard}>
              <h3>Transactions</h3>
              <div className={styles.value}>{program.transaction_count}</div>
              <div className={styles.label}>Total</div>
            </div>
            <div className={styles.statCard}>
              <h3>First Seen</h3>
              <div className={styles.value} style={{ fontSize: '1.1rem' }}>{program.first_seen_at ? new Date(program.first_seen_at).toLocaleString() : '—'}</div>
              <div className={styles.label}>Timestamp</div>
            </div>
            <div className={styles.statCard}>
              <h3>Last Seen</h3>
              <div className={styles.value} style={{ fontSize: '1.1rem' }}>{program.last_seen_at ? new Date(program.last_seen_at).toLocaleString() : '—'}</div>
              <div className={styles.label}>Timestamp</div>
            </div>
          </div>
        )}
      </section>

      {recent.length > 0 && (
        <section className={styles.searchSection}>
          <h2>Recent Transactions</h2>
          <table className={styles.transactionsTable}>
            <thead>
              <tr>
                <th>TxID</th>
                <th>Block</th>
                <th>Created</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((rt: any) => (
                <tr key={rt.txid}>
                  <td><a className={styles.hashButton} href={`/tx/${rt.txid}`}>{middleEllipsis(rt.txid, 8)}</a></td>
                  <td><a className={styles.hashButton} href={`/blocks/${rt.block_height}`}>{rt.block_height}</a></td>
                  <td>{rt.created_at ? new Date(rt.created_at).toLocaleString() : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </Layout>
  );
}
