import React, { useEffect, useState, useMemo } from 'react';
import { useRouter } from 'next/router';
import Layout from '../components/Layout';
import styles from '../styles/Home.module.css';

export default function SearchResultsPage() {
  const router = useRouter();
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || '';
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [results, setResults] = useState<any | null>(null);

  const q = useMemo(() => {
    const qp = typeof router.query.q === 'string' ? router.query.q : Array.isArray(router.query.q) ? router.query.q[0] : '';
    return (qp || '').trim();
  }, [router.query.q]);

  useEffect(() => {
    if (!router.isReady || !q) return;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/search?q=${encodeURIComponent(q)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        if (json?.bestGuess?.redirect && json?.bestGuess?.url) {
          router.replace(json.bestGuess.url);
          return;
        }
        setResults(json?.results || {});
      } catch (e: any) {
        setError('Search failed');
      } finally {
        setLoading(false);
      }
    })();
  }, [router.isReady, q, apiUrl, router]);

  const group = (key: string) => (Array.isArray(results?.[key]) ? results[key] : []);

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Search Results</h2>
        {q && <div className={styles.statusOther}>Query: <code style={{ opacity: 0.9 }}>{q}</code></div>}
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
      </section>

      {!loading && !error && results && (
        <>
          <section className={styles.searchSection}>
            <h3>Transactions</h3>
            {group('transactions').length === 0 ? (
              <div className={styles.statusOther}>No transactions</div>
            ) : (
              <ul className={styles.listPlain}>
                {group('transactions').map((t: any) => (
                  <li key={t.txid}><a className={styles.hashButton} href={t.url}>{t.txid}</a></li>
                ))}
              </ul>
            )}
          </section>

          <section className={styles.searchSection}>
            <h3>Blocks</h3>
            {group('blocks').length === 0 ? (
              <div className={styles.statusOther}>No blocks</div>
            ) : (
              <table className={styles.transactionsTable}>
                <thead>
                  <tr><th>Height</th><th>Hash</th></tr>
                </thead>
                <tbody>
                  {group('blocks').map((b: any) => (
                    <tr key={b.height}>
                      <td><a className={styles.hashButton} href={b.url}>{b.height}</a></td>
                      <td style={{ wordBreak: 'break-all', fontFamily: 'monospace' }}>{b.hash}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className={styles.searchSection}>
            <h3>Accounts</h3>
            {group('accounts').length === 0 ? (
              <div className={styles.statusOther}>No accounts</div>
            ) : (
              <ul className={styles.listPlain}>
                {group('accounts').map((a: any) => (
                  <li key={a.address}><a className={styles.hashButton} href={a.url}>{a.address}</a></li>
                ))}
              </ul>
            )}
          </section>

          <section className={styles.searchSection}>
            <h3>Programs</h3>
            {group('programs').length === 0 ? (
              <div className={styles.statusOther}>No programs</div>
            ) : (
              <ul className={styles.listPlain}>
                {group('programs').map((p: any) => (
                  <li key={p.programIdHex || p.programId}><a className={styles.hashButton} href={p.url}>{p.displayName || p.programId}</a></li>
                ))}
              </ul>
            )}
          </section>

          <section className={styles.searchSection}>
            <h3>Tokens</h3>
            {group('tokens').length === 0 ? (
              <div className={styles.statusOther}>No tokens</div>
            ) : (
              <table className={styles.transactionsTable}>
                <thead>
                  <tr><th>Mint</th><th>Symbol</th><th>Decimals</th></tr>
                </thead>
                <tbody>
                  {group('tokens').map((t: any) => (
                    <tr key={t.mint_hex || t.mint}>
                      <td style={{ wordBreak: 'break-all', fontFamily: 'monospace' }}>{t.mint}</td>
                      <td>{t.symbol || '—'}</td>
                      <td>{t.decimals ?? '—'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        </>
      )}
    </Layout>
  );
}
