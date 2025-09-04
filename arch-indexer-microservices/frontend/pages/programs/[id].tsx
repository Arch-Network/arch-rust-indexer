import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/router';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';
import { middleEllipsis } from '../../utils/format';

type Program = {
  program_id_hex: string;
  program_id_base58: string;
  transaction_count: number;
  first_seen_at?: string;
  last_seen_at?: string;
  display_name?: string | null;
};

function isHex(str: string): boolean {
  // Treat as hex only for exact 64-character (32-byte) hex strings
  return str.length === 64 && /^[0-9a-fA-F]+$/.test(str);
}

// Known mapped IDs we care about on the client for canonicalizing
const MAPPED: Record<string, string> = {
  // base58 -> mapped label used in routes
  'AplToken111111111111111111111111': 'AplToken111111111111111111111111',
  'AplAssociatedTokenAccount11111111111111111': 'AplAssociatedTokenAccount11111111111111111',
};

export default function ProgramDetailPage() {
  const router = useRouter();
  const id = router.query.id as string | undefined;
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || '';
  const [program, setProgram] = useState<Program | null>(null);
  const [recent, setRecent] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Canonicalize route on mount/change
  useEffect(() => {
    if (!id) return;
    // If hex, try to fetch program to obtain base58/display_name and redirect
    if (isHex(id)) {
      (async () => {
        try {
          const res = await fetch(`${apiUrl}/api/programs/${encodeURIComponent(id)}`);
          if (!res.ok) throw new Error('not found');
          const json = await res.json();
          const p = json.program || json;
          const base58 = p.program_id_base58 || '';
          const mapped = p.display_name && typeof p.display_name === 'string' ? p.display_name : undefined;
          const canonical = mapped && MAPPED[mapped] ? MAPPED[mapped] : (base58 || undefined);
          if (canonical && canonical !== id) {
            router.replace(`/programs/${encodeURIComponent(canonical)}`);
          } else {
            // If we cannot canonicalize, show 404
            setError('Program not found');
          }
        } catch {
          setError('Program not found');
        } finally {
          setLoading(false);
        }
      })();
      return;
    }
    // If base58 matches a mapped id, ensure we are at mapped path (already base58)
    if (MAPPED[id]) {
      if (id !== MAPPED[id]) {
        router.replace(`/programs/${encodeURIComponent(MAPPED[id])}`);
        return;
      }
    }
  }, [id, apiUrl, router]);

  useEffect(() => {
    if (!id || isHex(id)) return; // hex is handled by canonicalization above
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/programs/${encodeURIComponent(id)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        const p = json.program || json;
        setProgram({
          program_id_hex: p.program_id_hex || p.program_id,
          program_id_base58: p.program_id_base58 || id,
          transaction_count: p.transaction_count,
          first_seen_at: p.first_seen_at,
          last_seen_at: p.last_seen_at,
          display_name: p.display_name || null,
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
              <div className={styles.value} style={{ fontSize: '1rem', wordBreak: 'break-all' }}>
                {program.display_name || program.program_id_base58}
              </div>
              <div className={styles.label}>Identifier</div>
              {program.program_id_hex && (
                <div style={{ opacity: 0.6, fontSize: '0.8rem', wordBreak: 'break-all', marginTop: 4 }}>
                  {program.program_id_hex}
                </div>
              )}
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
