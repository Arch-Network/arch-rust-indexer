import React, { useEffect, useState } from 'react';
import Link from 'next/link';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';
import Pagination from '../../components/Pagination';
import { middleEllipsis } from '../../utils/format';


type Block = { height: number; hash: string; timestamp: string; transaction_count: number };

export default function BlocksPage() {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(1);
  const pageSize = 50;
  const [total, setTotal] = useState(0);

  useEffect(() => {
    (async () => {
      setLoading(true);
      try {
        const offset = (page - 1) * pageSize;
        const res = await fetch(`${apiUrl}/api/blocks?limit=${pageSize}&offset=${offset}`);
        const json = await res.json();
        setBlocks(json.blocks || []);
        setTotal(json.total_count ?? 0);
      } finally { setLoading(false); }
    })();
  }, [apiUrl, page]);

  return (
    <Layout>
      <section className={styles.blocksSection}>
        <h2>Blocks</h2>
        {loading ? <div className={styles.loading}>Loading…</div> : (
          <>
            <table className={styles.blocksTable}>
              <thead>
                <tr>
                  <th>Height</th>
                  <th>Hash</th>
                  <th>Timestamp</th>
                  <th>Tx</th>
                </tr>
              </thead>
              <tbody>
                {blocks.map((b) => (
                  <tr key={b.height}>
                    <td><Link href={`/blocks/${b.height}`} className={styles.hashButton}>{b.height}</Link></td>
                    <td><Link href={`/blocks/${b.hash}`} className={styles.hashButton}>{middleEllipsis(b.hash, 8)}</Link></td>
                    <td>{b.timestamp ? new Date(b.timestamp).toLocaleString() : '—'}</td>
                    <td>{b.transaction_count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <Pagination page={page} pageSize={pageSize} total={total} onPageChange={setPage} />
          </>
        )}
      </section>
    </Layout>
  );
}
