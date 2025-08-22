import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/router';
import Link from 'next/link';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

type Tx = { txid: string; block_height: number; status?: any; created_at: string; data?: any };

export default function TxDetailPage() {
  const router = useRouter();
  const txid = router.query.txid as string | undefined;
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [tx, setTx] = useState<Tx | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);

  useEffect(() => {
    if (!txid) return;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/transactions/${txid}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        setTx(json);
      } catch (e: any) {
        setError('Transaction not found');
      } finally {
        setLoading(false);
      }
    })();
  }, [txid, apiUrl]);

  const formatStatus = (s: any) => {
    if (!s) return 'PENDING';
    if (typeof s === 'string') {
      const up = s.toUpperCase();
      if (up.includes('PROCESSED') || up.includes('SUCCESS')) return 'SUCCESS';
      if (up.includes('FAIL')) return 'FAILED';
      if (up.includes('PEND')) return 'PENDING';
      return 'INFO';
    }
    const up = JSON.stringify(s).toUpperCase();
    if (up.includes('PROCESSED') || up.includes('SUCCESS')) return 'SUCCESS';
    if (up.includes('FAILED') || up.includes('ERROR')) return 'FAILED';
    if (up.includes('PENDING')) return 'PENDING';
    return 'INFO';
  };

  return (
    <Layout rightActions={<button className={styles.refreshButton} onClick={() => router.reload()}>Refresh</button>}>
      <section className={styles.searchSection}>
        <h2>Transaction Detail</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {tx && (
          <div className={styles.blockDetails}>
            <div className={styles.detailRow}><strong>TxID</strong> <span className={styles.hashValue}>{tx.txid}</span></div>
            <div className={styles.detailRow}><strong>Block</strong> <Link href={`/blocks/${tx.block_height}`} className={styles.hashButton}>{tx.block_height}</Link></div>
            <div className={styles.detailRow}><strong>Status</strong> {formatStatus(tx.status)}</div>
            <div className={styles.detailRow}><strong>Created</strong> {tx.created_at ? new Date(tx.created_at).toLocaleString() : '—'}</div>
            <div className={styles.detailRow}><strong>Actions</strong> <button className={styles.searchButton} onClick={() => setShowRaw(v => !v)}>{showRaw ? 'Hide JSON' : 'Show JSON'}</button></div>
          </div>
        )}
      </section>
      {showRaw && tx?.data && (
        <section className={styles.searchSection}>
          <h2>Raw JSON</h2>
          <pre className={styles.rawJson}>{JSON.stringify(tx.data, null, 2)}</pre>
        </section>
      )}
      <div className={styles.searchTips}><Link href="/tx">← Back to Transactions</Link></div>
    </Layout>
  );
}
