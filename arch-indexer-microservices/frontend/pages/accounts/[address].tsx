import { useRouter } from 'next/router';
import { useEffect, useMemo, useState } from 'react';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';

type AccountSummary = {
  address: string;
  address_hex: string;
  first_seen: string | null;
  last_seen: string | null;
  transaction_count: number;
  lamports_balance?: number | null;
};

type TxRowV2 = {
  txid: string;
  block_height: number;
  created_at: string;
  status?: string;
  fee_payer?: string;
  value_arch?: number;
  fee_estimated_arch?: number | null;
  programs?: string[];
  instructions?: string[];
};

type ProgramRow = { program_id: string; program_id_base58?: string; transaction_count: number };

type TokenBalance = {
  mint_address: string;
  mint_address_hex: string;
  balance: string;
  decimals: number;
  owner_address?: string;
  program_id: string;
  program_name?: string;
  supply?: string;
  is_frozen?: boolean;
  last_updated: string;
};

function safeDate(v?: string | null): string {
  if (!v) return '—';
  const d = new Date(v);
  return isNaN(d.getTime()) ? '—' : d.toLocaleString();
}

function timeAgo(v?: string): string {
  if (!v) return '—';
  const t = new Date(v).getTime();
  if (isNaN(t)) return '—';
  const s = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}

function trimMiddle(str: string, head = 8, tail = 6): string {
  if (!str) return '';
  if (str.length <= head + tail + 1) return str;
  return `${str.slice(0, head)}…${str.slice(-tail)}`;
}

function normalizeStatus(status?: string): { text: string; cls: string } {
  if (!status) return { text: '—', cls: styles.statusOther };
  let s = status;
  if (status.trim().startsWith('{')) {
    try { const obj = JSON.parse(status); s = obj.type || status; } catch {}
  }
  const up = String(s).toLowerCase();
  if (up.includes('success') || up.includes('processed')) return { text: 'Processed', cls: styles.statusSuccess };
  if (up.includes('fail') || up.includes('error')) return { text: 'Failed', cls: styles.statusOther };
  if (up.includes('pending')) return { text: 'Pending', cls: styles.statusInfo };
  return { text: s, cls: styles.statusOther };
}

export default function AccountPage() {
  const router = useRouter();
  const { address } = router.query as { address?: string };
  const [summary, setSummary] = useState<AccountSummary | null>(null);
  const [txs, setTxs] = useState<TxRowV2[]>([]);
  const [page, setPage] = useState(1);
  const [limit, setLimit] = useState(25);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<'tx' | 'programs' | 'balances'>('tx');
  const [programs, setPrograms] = useState<ProgramRow[]>([]);
  const [tokenBalances, setTokenBalances] = useState<TokenBalance[]>([]);
  const [balancesPage, setBalancesPage] = useState(1);
  const [balancesLimit, setBalancesLimit] = useState(25);
  const [balancesTotal, setBalancesTotal] = useState(0);

  const baseApi = useMemo(() => {
    if (typeof window === 'undefined') return '';
    return process.env.NEXT_PUBLIC_API_URL || '';
  }, []);

  useEffect(() => {
    if (!address) return;
    setLoading(true);
    setError(null);
    Promise.all([
      fetch(`${baseApi}/api/accounts/${address}`).then(r => r.ok ? r.json() : Promise.reject(r.statusText)),
      fetch(`${baseApi}/api/accounts/${address}/transactions/v2?limit=${limit}&page=${page}`).then(r => r.ok ? r.json() : Promise.reject(r.statusText)),
      fetch(`${baseApi}/api/accounts/${address}/programs`).then(r => r.ok ? r.json() : Promise.resolve([])),
    ])
      .then(([summaryJson, txJson, progJson]) => {
        setSummary(summaryJson);
        setTxs((txJson?.transactions as TxRowV2[]) || []);
        setPrograms((progJson as ProgramRow[]) || []);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [address, page, limit, baseApi]);

  // Fetch token balances when balances tab is selected
  useEffect(() => {
    if (!address || tab !== 'balances') return;
    setLoading(true);
    setError(null);
    fetch(`${baseApi}/api/accounts/${address}/token-balances?limit=${balancesLimit}&page=${balancesPage}`)
      .then(r => r.ok ? r.json() : Promise.reject(r.statusText))
      .then((data) => {
        setTokenBalances((data?.balances as TokenBalance[]) || []);
        setBalancesTotal(data?.total || 0);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [address, tab, balancesPage, balancesLimit, baseApi]);

  const copy = (value: string) => {
    if (navigator?.clipboard) {
      navigator.clipboard.writeText(value);
    }
  };

  if (!address) return <div style={{ padding: 20 }}>Loading…</div>;

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Account</h2>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <code style={{ fontSize: 14 }}>{address}</code>
        <button onClick={() => copy(address)}>Copy</button>
      </div>

      {error && <div className={styles.statusOther} style={{ color: 'red', marginTop: 12 }}>{error}</div>}

      {summary && (
        <div style={{ marginTop: 16, display: 'grid', gridTemplateColumns: 'repeat(auto-fit,minmax(240px,1fr))', gap: 12 }}>
          <div style={{ border: '1px solid #222', borderRadius: 8, padding: 12 }}>
            <div style={{ opacity: 0.7, fontSize: 12 }}>First Seen</div>
            <div>{safeDate(summary.first_seen)}</div>
          </div>
          <div style={{ border: '1px solid #222', borderRadius: 8, padding: 12 }}>
            <div style={{ opacity: 0.7, fontSize: 12 }}>Last Seen</div>
            <div>{safeDate(summary.last_seen)}</div>
          </div>
          <div style={{ border: '1px solid #222', borderRadius: 8, padding: 12 }}>
            <div style={{ opacity: 0.7, fontSize: 12 }}>Transactions</div>
            <div>{(summary.transaction_count ?? 0).toLocaleString()}</div>
          </div>
          <div style={{ border: '1px solid #222', borderRadius: 8, padding: 12 }}>
            <div style={{ opacity: 0.7, fontSize: 12 }}>Lamports (computed)</div>
            <div>{(summary.lamports_balance ?? 0).toLocaleString()}</div>
          </div>
          <div style={{ border: '1px solid #222', borderRadius: 8, padding: 12 }}>
            <div style={{ opacity: 0.7, fontSize: 12 }}>Address (hex)</div>
            <div style={{ wordBreak: 'break-all' }}>
              <code style={{ fontSize: 12 }}>{summary.address_hex}</code>
            </div>
          </div>
        </div>
      )}
      {/* Tabs */}
      <div style={{ marginTop: 24, display: 'flex', gap: 8 }}>
        <button onClick={() => setTab('tx')} style={{ padding: '6px 10px', borderRadius: 6, border: '1px solid #333', background: tab==='tx' ? '#222' : 'transparent', color: '#e6edf3' }}>Transactions</button>
        <button onClick={() => setTab('programs')} style={{ padding: '6px 10px', borderRadius: 6, border: '1px solid #333', background: tab==='programs' ? '#222' : 'transparent', color: '#e6edf3' }}>Programs</button>
        <button onClick={() => setTab('balances')} style={{ padding: '6px 10px', borderRadius: 6, border: '1px solid #333', background: tab==='balances' ? '#222' : 'transparent', color: '#e6edf3' }}>Token Balances</button>
      </div>

      {/* Tab Content */}
      {tab === 'tx' && (
        <>
          <h2 style={{ marginTop: 24 }}>Recent Transactions</h2>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 16 }}>
            <label>Page</label>
            <input
              type="number"
              min={1}
              value={page}
              onChange={(e) => setPage(Math.max(1, parseInt(e.target.value || '1', 10)))}
              style={{ width: 80 }}
            />
            <label>Limit</label>
            <select value={limit} onChange={(e) => setLimit(parseInt(e.target.value, 10))}>
              <option value={10}>10</option>
              <option value={25}>25</option>
              <option value={50}>50</option>
              <option value={100}>100</option>
            </select>
          </div>
          <div className={styles.searchSection} style={{ overflowX: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Signature</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Block</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Time</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Instructions</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>By</th>
                  <th style={{ textAlign: 'right', borderBottom: '1px solid #333', padding: '8px 4px' }}>Value (ARCH)</th>
                  <th style={{ textAlign: 'right', borderBottom: '1px solid #333', padding: '8px 4px' }}>Fee (ARCH)</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Programs</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Status</th>
                </tr>
              </thead>
              <tbody>
                {txs.map((t) => {
                  const status = normalizeStatus(t.status);
                  return (
                    <tr key={t.txid}>
                      <td style={{ padding: '8px 4px', fontFamily: 'monospace', whiteSpace: 'nowrap' }}>
                        <a href={`/tx/${t.txid}`} style={{ color: '#5cf' }} title={t.txid}>{trimMiddle(t.txid)}</a>
                      </td>
                      <td style={{ padding: '8px 4px' }}>
                        <a href={`/blocks/${t.block_height}`} style={{ color: '#5cf' }}>{t.block_height}</a>
                      </td>
                      <td style={{ padding: '8px 4px' }} title={safeDate(t.created_at)}>{timeAgo(t.created_at)}</td>
                      <td style={{ padding: '8px 4px', maxWidth: 320 }}>
                        {(t.instructions || []).slice(0,6).map((c, i) => (
                          <span key={i} className={styles.badge} style={{ marginRight: 6, marginBottom: 4, display: 'inline-block' }}>{c}</span>
                        ))}
                      </td>
                      <td style={{ padding: '8px 4px', fontFamily: 'monospace' }} title={t.fee_payer || ''}>{t.fee_payer ? trimMiddle(t.fee_payer, 6, 6) : '—'}</td>
                      <td style={{ padding: '8px 4px', textAlign: 'right' }}>{(t.value_arch ?? 0).toLocaleString(undefined, { maximumFractionDigits: 9 })}</td>
                      <td style={{ padding: '8px 4px', textAlign: 'right' }}>{t.fee_estimated_arch != null ? `~${t.fee_estimated_arch.toLocaleString(undefined, { maximumFractionDigits: 9 })}` : '—'}</td>
                      <td style={{ padding: '8px 4px', maxWidth: 240 }}>
                        {(t.programs || []).slice(0,3).map((p, i) => (
                          <a key={i} href={`/programs/${encodeURIComponent(p)}`} className={styles.hashButton} style={{ marginRight: 6, marginBottom: 4, display: 'inline-block' }}>{trimMiddle(p, 6, 6)}</a>
                        ))}
                        {((t.programs || []).length > 3) && <span className={styles.muted}>+{(t.programs!.length - 3)}</span>}
                      </td>
                      <td style={{ padding: '8px 4px' }}><span className={status.cls}>{status.text}</span></td>
                    </tr>
                  );
                })}
                {!loading && txs.length === 0 && (
                  <tr>
                    <td colSpan={9} style={{ padding: 12, opacity: 0.7 }}>No transactions found.</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </>
      )}

      {tab === 'programs' && (
        <div className={styles.searchSection} style={{ marginTop: 24 }}>
          <h2>Programs</h2>
          <div style={{ overflowX: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Program ID</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Transactions</th>
                </tr>
              </thead>
              <tbody>
                {programs.map((p) => (
                  <tr key={p.program_id}>
                    <td style={{ padding: '8px 4px', fontFamily: 'monospace' }}>
                      <a href={`/programs/${p.program_id_base58 || p.program_id}`} style={{ color: '#5cf' }}>
                        {p.program_id_base58 || p.program_id}
                      </a>
                    </td>
                    <td style={{ padding: '8px 4px' }}>{(p.transaction_count ?? 0).toLocaleString()}</td>
                  </tr>
                ))}
                {!loading && programs.length === 0 && (
                  <tr>
                    <td colSpan={2} style={{ padding: 12, opacity: 0.7 }}>No program interactions found.</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {tab === 'balances' && (
        <>
          <h2 style={{ marginTop: 24 }}>Token Balances</h2>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 16 }}>
            <label>Page</label>
            <input
              type="number"
              min={1}
              value={balancesPage}
              onChange={(e) => setBalancesPage(Math.max(1, parseInt(e.target.value || '1', 10)))}
              style={{ width: 80 }}
            />
            <label>Limit</label>
            <select value={balancesLimit} onChange={(e) => setBalancesLimit(parseInt(e.target.value, 10))}>
              <option value={10}>10</option>
              <option value={25}>25</option>
              <option value={50}>50</option>
              <option value={100}>100</option>
            </select>
            <span style={{ marginLeft: 16, opacity: 0.7 }}>
              Total: {balancesTotal.toLocaleString()}
            </span>
          </div>
          <div className={styles.searchSection} style={{ overflowX: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Mint Address</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Balance</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Program</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Supply</th>
                  <th style={{ textAlign: 'left', borderBottom: '1px solid #333', padding: '8px 4px' }}>Last Updated</th>
                </tr>
              </thead>
              <tbody>
                {tokenBalances.map((balance) => (
                  <tr key={`${balance.mint_address}-${balance.program_id}`}>
                    <td style={{ padding: '8px 4px', fontFamily: 'monospace' }}>
                      <div style={{ fontSize: 12 }}>
                        <div>{balance.mint_address}</div>
                        <div style={{ opacity: 0.6, fontSize: 10 }}>{balance.mint_address_hex}</div>
                      </div>
                    </td>
                    <td style={{ padding: '8px 4px' }}>
                      <div>
                        <div>{parseFloat(balance.balance).toLocaleString()}</div>
                        {balance.decimals > 0 && (
                          <div style={{ opacity: 0.6, fontSize: 12 }}>
                            {parseFloat(balance.balance) / Math.pow(10, balance.decimals)}
                          </div>
                        )}
                      </div>
                    </td>
                    <td style={{ padding: '8px 4px' }}>
                      <div style={{ fontSize: 12 }}>
                        <div>{balance.program_name || balance.program_id}</div>
                        <div style={{ opacity: 0.6, fontSize: 10 }}>{balance.program_id}</div>
                      </div>
                    </td>
                    <td style={{ padding: '8px 4px' }}>
                      {balance.supply ? (
                        <div>
                          <div>{parseFloat(balance.supply).toLocaleString()}</div>
                          {balance.decimals > 0 && (
                            <div style={{ opacity: 0.6, fontSize: 12 }}>
                              {parseFloat(balance.supply) / Math.pow(10, balance.decimals)}
                            </div>
                          )}
                        </div>
                      ) : (
                        <span style={{ opacity: 0.5 }}>—</span>
                      )}
                    </td>
                    <td style={{ padding: '8px 4px' }}>{safeDate(balance.last_updated)}</td>
                  </tr>
                ))}
                {!loading && tokenBalances.length === 0 && (
                  <tr>
                    <td colSpan={5} style={{ padding: 12, opacity: 0.7 }}>
                      No token balances found for this account.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </>
      )}
      </section>
    </Layout>
  );
}
