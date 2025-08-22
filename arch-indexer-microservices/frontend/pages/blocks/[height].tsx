import React, { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/router';
import Link from 'next/link';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

type Block = {
  height: number;
  hash: string;
  timestamp: string;
  transaction_count: number;
  bitcoin_block_height?: number;
  transactions?: Array<{ txid: string; block_height: number; created_at?: string; status?: any; bitcoin_txids?: string[] }>;
};

export default function BlockDetailPage() {
  const router = useRouter();
  const id = router.query.height as string | undefined;
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [block, setBlock] = useState<Block | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const isNumeric = useMemo(() => !!id && /^\d+$/.test(id), [id]);

  useEffect(() => {
    if (!id) return;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const endpoint = isNumeric ? `${apiUrl}/api/blocks/height/${id}` : `${apiUrl}/api/blocks/${id}`;
        const res = await fetch(endpoint);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        setBlock(json);
      } catch (e: any) {
        setError('Block not found');
      } finally {
        setLoading(false);
      }
    })();
  }, [id, isNumeric, apiUrl]);

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Block Detail</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {block && (
          <div className={styles.blockDetails}>
            <div className={styles.detailRow}><strong>Height</strong> {block.height}</div>
            <div className={styles.detailRow}><strong>Hash</strong> <span className={styles.hashValue}>{block.hash}</span></div>
            <div className={styles.detailRow}><strong>Timestamp</strong> {block.timestamp ? new Date(block.timestamp).toLocaleString() : '—'}</div>
            <div className={styles.detailRow}><strong>Transactions</strong> {block.transaction_count}</div>
            {typeof block.bitcoin_block_height === 'number' && (
              <div className={styles.detailRow}>
                <strong>Bitcoin Block</strong>
                <a
                  className={styles.hashButton}
                  href={`${(process.env.NEXT_PUBLIC_BTC_NETWORK || 'testnet4') === 'mainnet' ? 'https://mempool.space' : 'https://mempool.space/testnet4'}/block/${block.bitcoin_block_height}`}
                  target="_blank" rel="noreferrer"
                >
                  {block.bitcoin_block_height}
                </a>
              </div>
            )}
          </div>
        )}
      </section>

      {block?.transactions && block.transactions.length > 0 && (
        <section className={styles.transactionsSection}>
          <h2>Transactions</h2>
          <table className={styles.transactionsTable}>
            <thead>
              <tr>
                <th>TxID</th>
                <th>BTC Tx</th>
                <th>Created</th>
              </tr>
            </thead>
            <tbody>
              {block.transactions.map((t) => {
                const btcNet = (process.env.NEXT_PUBLIC_BTC_NETWORK || 'testnet4') === 'mainnet' ? 'https://mempool.space' : 'https://mempool.space/testnet4';
                const btcTx = t.bitcoin_txids && t.bitcoin_txids.length > 0 ? t.bitcoin_txids[0] : undefined;
                return (
                <tr key={t.txid}>
                  <td>
                    <Link href={`/tx/${t.txid}`} className={styles.hashButton}>{t.txid.slice(0,16)}…</Link>
                  </td>
                  <td>
                    {btcTx ? (
                      <a href={`${btcNet}/tx/${btcTx}`} className={styles.hashButton} target="_blank" rel="noreferrer">{btcTx.slice(0,16)}…</a>
                    ) : (
                      '—'
                    )}
                  </td>
                  <td>{t.created_at ? new Date(t.created_at).toLocaleString() : '—'}</td>
                </tr>
                );
              })}
            </tbody>
          </table>
        </section>
      )}

      <div className={styles.searchTips}><Link href="/blocks">← Back to Blocks</Link></div>
    </Layout>
  );
}
