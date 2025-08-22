import React, { useEffect, useState } from 'react';
import Link from 'next/link';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

type Tx = { txid: string; block_height: number; status?: any; created_at: string };

export default function TransactionsPage() {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [txs, setTxs] = useState<Tx[]>([]);

  useEffect(() => {
    (async () => {
      try {
        const res = await fetch(`${apiUrl}/api/transactions?limit=50&offset=0`);
        const json = await res.json();
        setTxs(Array.isArray(json) ? json : (json.transactions || []));
      } catch {}
    })();
  }, [apiUrl]);

  const formatStatus = (s: any) => {
    if (!s) return 'PENDING';
    if (typeof s === 'string') {
      const up = s.toUpperCase();
      if (up.includes('SUCCESS') || up.includes('PROCESSED')) return 'SUCCESS';
      if (up.includes('FAIL')) return 'FAILED';
      if (up.includes('PEND')) return 'PENDING';
      return 'INFO';
    }
    if (typeof s === 'object') {
      // Case-insensitive scan of the object for known status tokens
      const up = JSON.stringify(s).toUpperCase();
      if (up.includes('PROCESSED') || up.includes('SUCCESS')) return 'SUCCESS';
      if (up.includes('FAILED') || up.includes('ERROR')) return 'FAILED';
      if (up.includes('PENDING')) return 'PENDING';
      return 'INFO';
    }
    return 'INFO';
  };

  return (
    <Layout>
      <section className={styles.transactionsSection}>
        <h2>Transactions</h2>
        <table className={styles.transactionsTable}>
          <thead>
            <tr>
              <th>TxID</th>
              <th>Block</th>
              <th>Status</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {txs.map((t) => (
              <tr key={t.txid}>
                <td><Link href={`/tx/${t.txid}`} className={styles.hashButton}>{t.txid.slice(0,16)}…</Link></td>
                <td><Link href={`/blocks/${t.block_height}`} className={styles.hashButton}>{t.block_height}</Link></td>
                <td>{formatStatus(t.status)}</td>
                <td>{t.created_at ? new Date(t.created_at).toLocaleString() : '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </Layout>
  );
}
