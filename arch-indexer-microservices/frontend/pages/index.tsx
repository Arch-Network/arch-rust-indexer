import React, { useState, useEffect } from 'react';
import Head from 'next/head';
import styles from '../styles/Home.module.css';
import Layout from '../components/Layout';
import BlockScroller from '../components/BlockScroller';
import DonutProgress from '../components/DonutProgress';
import Sparkline from '../components/Sparkline';
import { middleEllipsis } from '../utils/format';
import dynamic from 'next/dynamic';
const ProgramHeatstrip = dynamic(() => import('../components/ProgramHeatstrip'), { ssr: false });

interface NetworkStats {
  total_blocks: number;
  total_transactions: number;
  latest_block_height: number;
  // Extended fields from API
  block_height?: number;
  slot_height?: number;
  current_tps?: number;
  average_tps?: number;
  peak_tps?: number;
  daily_transactions?: number;
}

interface Block {
  height: number;
  hash: string;
  timestamp: string;
  transaction_count: number;
}

interface Transaction {
  txid: string;
  block_height: number;
  status: any;
  created_at: string;
}

export default function Home() {
  const [stats, setStats] = useState<NetworkStats | null>(null);
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedBlock, setSelectedBlock] = useState<Block | null>(null);
  const [selectedTransaction, setSelectedTransaction] = useState<Transaction | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [memeMode, setMemeMode] = useState(false);
  const [mempoolStats, setMempoolStats] = useState<any | null>(null);
  const [recentMempool, setRecentMempool] = useState<any[]>([]);
  const [isTxDrawerOpen, setIsTxDrawerOpen] = useState(false);
  const [drawerTx, setDrawerTx] = useState<Transaction | null>(null);
  const [showRawJson, setShowRawJson] = useState(false);
  const [programQuery, setProgramQuery] = useState('');
  const [programResult, setProgramResult] = useState<any | null>(null);
  const [programLoading, setProgramLoading] = useState(false);
  const [programError, setProgramError] = useState<string | null>(null);
  const [timezone, setTimezone] = useState<string>('local');

  // Get API URL from environment or fallback to localhost
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';

  useEffect(() => {
    loadNetworkStats();
    loadBlocks();
    loadTransactions();
    connectWebSocket();
    createMatrixRain();
    try {
      const savedTz = typeof window !== 'undefined' ? window.localStorage.getItem('tz') : null;
      if (savedTz) {
        setTimezone(savedTz);
      } else {
        const localTz = Intl.DateTimeFormat().resolvedOptions().timeZone;
        setTimezone(localTz || 'UTC');
      }
    } catch {}
  }, []);

  // Auto-refresh core summaries and lists
  useEffect(() => {
    const id = setInterval(() => {
      loadNetworkStats();
      loadBlocks();
      loadTransactions();
    }, 10000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    // Apply class and persist toggle
    const html = document.querySelector('html');
    if (!html) return;
    if (memeMode) {
      html.classList.add('meme-mode');
    } else {
      html.classList.remove('meme-mode');
    }
    try {
      if (typeof window !== 'undefined') {
        window.localStorage.setItem('memeMode', memeMode ? '1' : '0');
      }
    } catch {
      // ignore storage errors
    }
  }, [memeMode]);

  useEffect(() => {
    const interval = setInterval(() => {
      loadMempool();
    }, 5000);
    loadMempool();
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === 'd') {
        if (selectedTransaction) {
          setDrawerTx(selectedTransaction);
          setIsTxDrawerOpen(true);
        }
      }
      if (e.key === 'Escape') {
        setIsTxDrawerOpen(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selectedTransaction]);

  const openTxDrawer = (tx: Transaction) => {
    setDrawerTx(tx);
    setIsTxDrawerOpen(true);
  };

  const decodeProgramSteps = (tx: any) => {
    try {
      const instructions = tx?.data?.message?.instructions;
      if (!Array.isArray(instructions)) return [];
      return instructions.map((ins: any, idx: number) => ({
        index: idx,
        programId: ins.program_id || ins.programId || 'unknown',
        opcode: ins.opcode || ins.name || 'instruction',
        meta: ins.accounts ? `${ins.accounts.length} accounts` : '',
        raw: ins,
      }));
    } catch {
      return [];
    }
  };

  const createMatrixRain = () => {
    const matrixBg = document.createElement('div');
    matrixBg.className = 'matrix-bg';
    
    for (let i = 0; i < 30; i++) {
      const char = document.createElement('div');
      char.className = 'matrix-char';
      char.textContent = String.fromCharCode(0x30A0 + Math.random() * 96);
      char.style.left = Math.random() * 100 + '%';
      char.style.animationDuration = (Math.random() * 10 + 5) + 's';
      char.style.animationDelay = Math.random() * 5 + 's';
      matrixBg.appendChild(char);
    }
    
    document.body.appendChild(matrixBg);
  };

  const loadNetworkStats = async () => {
    try {
      const response = await fetch(`${apiUrl}/api/network/stats`);
      const data = await response.json();
      setStats(data);
    } catch (error) {
      console.error('Failed to load network stats:', error);
    }
  };

  const loadBlocks = async () => {
    try {
      const response = await fetch(`${apiUrl}/api/blocks?limit=20&offset=0`);
      const data = await response.json();
      setBlocks(data.blocks || []);
      setIsLoading(false);
    } catch (error) {
      console.error('Failed to load blocks:', error);
      setIsLoading(false);
    }
  };

  const loadTransactions = async () => {
    try {
      const response = await fetch(`${apiUrl}/api/transactions?limit=20&offset=0`);
      const data = await response.json();
      setTransactions(Array.isArray(data) ? data : (data.transactions || []));
    } catch (error) {
      console.error('Failed to load transactions:', error);
    }
  };

  const loadMempool = async () => {
    try {
      const [statsRes, recentRes] = await Promise.all([
        fetch(`${apiUrl}/api/mempool/stats`),
        fetch(`${apiUrl}/api/mempool/recent`)
      ]);
      const stats = await statsRes.json();
      const recent = await recentRes.json();
      setMempoolStats(stats);
      setRecentMempool(Array.isArray(recent) ? recent : []);
    } catch (e) {
      // ignore ticker errors
    }
  };

  const connectWebSocket = () => {
    // WebSocket connection logic will go here
  };

  const performSearch = async () => {
    if (!searchQuery.trim()) return;
    
    try {
      const response = await fetch(`${apiUrl}/api/search?term=${encodeURIComponent(searchQuery)}`);
      const data = await response.json();
      
      if (data && data.data) {
        // Enhanced search results with more details
        const enhancedResult = {
          type: data.type,
          data: data.data,
          timestamp: new Date().toISOString(),
          query: searchQuery
        };
        setSearchResults([enhancedResult]);
      } else {
        setSearchResults([]);
      }
    } catch (error) {
      console.error('Search failed:', error);
      setSearchResults([]);
    }
  };

  const fetchProgram = async () => {
    const pid = programQuery.trim();
    if (!pid) return;
    setProgramLoading(true);
    setProgramError(null);
    try {
      const res = await fetch(`${apiUrl}/api/programs/${encodeURIComponent(pid)}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      setProgramResult(data);
    } catch (e: any) {
      setProgramError('Program not found or server error');
      setProgramResult(null);
    } finally {
      setProgramLoading(false);
    }
  };

  const renderSearchResult = (result: any) => {
    if (result.type === 'block') {
      return (
        <div className={styles.searchResult}>
          <h4>BLOCK DATA RETRIEVED</h4>
          <div className={styles.blockDetails}>
            <div className={styles.detailRow}>
              <strong>BLOCK HEIGHT:</strong> {result.data.height}
            </div>
            <div className={styles.detailRow}>
              <strong>BLOCK HASH:</strong> 
              <span className={styles.hashValue}>{result.data.hash}</span>
            </div>
            <div className={styles.detailRow}>
              <strong>TIMESTAMP:</strong> {formatTimestamp(result.data.timestamp)}
            </div>
            <div className={styles.detailRow}>
              <strong>TRANSACTIONS:</strong> {result.data.transaction_count || '0'}
            </div>
            {Array.isArray(result.data.transactions) && result.data.transactions.length > 0 && (
              <div className={styles.detailRow}>
                <strong>TX LIST:</strong>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                  {result.data.transactions.map((tx: any) => (
                    <button key={tx.txid} className={styles.hashButton} onClick={() => { setSelectedTransaction(tx); openTxDrawer(tx); }}>
                      {middleEllipsis(tx.txid, 8)}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <div className={styles.detailRow}>
              <strong>BLOCK SIZE:</strong> {result.data.block_size_bytes != null ? `${result.data.block_size_bytes} bytes` : 'UNKNOWN'}
            </div>
            <div className={styles.detailRow}>
              <strong>PREVIOUS BLOCK:</strong> 
              {result.data.previous_block_hash ? (
                <span className={styles.hashValue}>{middleEllipsis(result.data.previous_block_hash, 8)}</span>
              ) : 'GENESIS BLOCK'}
            </div>
            {/* Merkle root removed from API; intentionally not shown */}
          </div>
          <div className={styles.queryMeta}>
            <small>QUERY EXECUTED: {result.query} | RETRIEVED: {formatTimestamp(result.timestamp)}</small>
          </div>
        </div>
      );
    } else if (result.type === 'transaction') {
      return (
        <div className={styles.searchResult}>
          <h4>TRANSACTION DATA RETRIEVED</h4>
          <div className={styles.txDetails}>
            <div className={styles.detailRow}>
              <strong>TRANSACTION ID:</strong> 
              <span className={styles.hashValue}>{result.data.txid}</span>
            </div>
            <div className={styles.detailRow}>
              <strong>BLOCK HEIGHT:</strong> {result.data.block_height}
            </div>
            <div className={styles.detailRow}>
              <strong>STATUS:</strong> {formatTransactionStatus(result.data.status)}
            </div>
            <div className={styles.detailRow}>
              <strong>CREATED:</strong> {formatTimestamp(result.data.created_at)}
            </div>
            <div className={styles.detailRow}>
              <strong>FEE:</strong> {result.data.fee ? `${result.data.fee} lamports` : 'UNKNOWN'}
            </div>
          </div>
          <div className={styles.queryMeta}>
            <small>QUERY EXECUTED: {result.query} | RETRIEVED: {formatTimestamp(result.timestamp)}</small>
          </div>
        </div>
      );
    } else {
      return (
        <div className={styles.searchResult}>
          <h4>UNKNOWN DATA TYPE</h4>
          <div className={styles.unknownData}>
            <pre>{JSON.stringify(result.data, null, 2)}</pre>
          </div>
          <div className={styles.queryMeta}>
            <small>QUERY EXECUTED: {result.query} | RETRIEVED: {formatTimestamp(result.timestamp)}</small>
          </div>
        </div>
      );
    }
  };

  const formatTimestamp = (timestamp: string) => {
    if (!timestamp) return 'UNKNOWN';
    try {
      let date;
      if (typeof timestamp === 'string') {
        // Handle PostgreSQL overflow timestamps
        if (timestamp.includes('+') && timestamp.includes('-')) {
          const match = timestamp.match(/\+(\d+)-(\d+)-(\d+)T(\d+):(\d+):(\d+)Z/);
          if (match) {
            const [, year] = match;
            if (parseInt(year) > 9999) {
              return 'INVALID DATE (OVERFLOW)';
            }
          }
        }
        date = new Date(timestamp);
      }
      
      if (isNaN(date.getTime())) {
        return 'INVALID DATE';
      }
      
      const tz = timezone && timezone !== 'local' ? timezone : undefined;
      return date.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: true,
        timeZone: tz,
        timeZoneName: 'short'
      }).toUpperCase();
    } catch (error) {
      return 'FORMAT ERROR';
    }
  };

  const formatTransactionStatus = (status: any) => {
    try {
      if (!status) return <span className={styles.statusPending}>PENDING</span>;
      // Normalize to uppercase string for robust matching across shapes
      const up = (typeof status === 'string' ? status : JSON.stringify(status)).toUpperCase();
      if (up.includes('PROCESSED') || up.includes('SUCCESS')) {
        return <span className={styles.statusSuccess}>[SUCCESS] EXECUTED</span>;
      }
      if (up.includes('FAIL')) {
        return <span className={styles.statusFailed}>[FAILED]</span>;
      }
      if (up.includes('PEND')) {
        return <span className={styles.statusPending}>[PENDING] QUEUED</span>;
      }
      return <span className={styles.statusOther}>[INFO]</span>;
    } catch {
      return <span className={styles.statusOther}>[INFO]</span>;
    }
  };

  const calculateSyncProgress = () => {
    if (!stats) return { percentage: 0, synced: 0, total: 0 };
    const synced = stats.total_blocks;
    const total = stats.latest_block_height + 1;
    const percentage = Math.round((synced / total) * 100);
    return { percentage, synced, total };
  };

  const syncProgress = calculateSyncProgress();

  return (
    <Layout
      rightActions={(
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <a className={styles.refreshButton} href="/settings">Settings</a>
          <button className={styles.refreshButton} onClick={() => setMemeMode(v => !v)}>
            {memeMode ? 'Meme Mode: ON' : 'Meme Mode: OFF'}
          </button>
        </div>
      )}
    >
      <Head>
        <title>Arch Explorer • Cypherpunk Mode</title>
        <meta name="description" content="Fresh, usable blockchain explorer for Arch" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      {/* Program Activity Heatstrip */}
      <ProgramHeatstrip apiUrl={apiUrl} height={80} />

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0,1fr))', gap: 12, marginBottom: 18 }}>
          <div className={styles.statCard}>
            <h3>Current TPS</h3>
            <div className={styles.value}>{(stats?.current_tps ?? 0).toFixed(2)}</div>
            <div className={styles.label}>Avg: {(stats?.average_tps ?? 0).toFixed(2)} | Peak: {(stats?.peak_tps ?? 0).toFixed(2)}</div>
          </div>
          <div className={styles.statCard}>
            <h3>Mempool Size</h3>
            <div className={styles.value}>{mempoolStats?.total_transactions ?? 0}</div>
            <div className={styles.label}>Pending: {mempoolStats?.pending_count ?? 0}</div>
          </div>
          <div className={styles.statCard}>
            <h3>Avg Fee Priority</h3>
            <div className={styles.value}>{mempoolStats?.avg_fee_priority?.toFixed?.(0) ?? '—'}</div>
            <div className={styles.label}>Total Bytes: {mempoolStats?.total_size_bytes ?? 0}</div>
          </div>
          <div className={styles.statCard}>
            <h3>Newest Pending</h3>
            <div className={styles.value}>{mempoolStats?.newest_transaction ? formatTimestamp(mempoolStats.newest_transaction) : '—'}</div>
            <div className={styles.label}>Oldest: {mempoolStats?.oldest_transaction ? formatTimestamp(mempoolStats.oldest_transaction) : '—'}</div>
          </div>
        </div>

        {/* Live mempool ticker */}
        <div style={{ overflow: 'hidden', border: '1px solid rgba(255,255,255,0.06)', background: '#0a0c10', marginBottom: 20 }}>
          <div style={{ display: 'flex', gap: 24, padding: '8px 12px', animation: 'ticker 40s linear infinite', whiteSpace: 'nowrap' }}>
            {recentMempool.slice(0, 30).map((m) => (
              <span key={m.txid} style={{ color: 'var(--accent-2)', marginRight: 24 }}>
                TX {middleEllipsis(m.txid, 6)} ·· fee {m.fee_priority ?? '—'} · {m.size_bytes ?? '—'} bytes
              </span>
            ))}
          </div>
        </div>

        <div className={styles.statsGrid}>
          <div className={styles.statCard}>
            <h3>TPS</h3>
            <div className={styles.value}>{(stats?.current_tps ?? 0).toFixed(2)}</div>
            <div className={styles.label}>Avg {stats?.average_tps?.toFixed?.(2) ?? '0.00'} | Peak {stats?.peak_tps?.toFixed?.(2) ?? '0.00'}</div>
            <div style={{ marginTop: 8 }}>
              <Sparkline data={[stats?.average_tps ?? 0, stats?.current_tps ?? 0, (stats?.current_tps ?? 0) * 1.1, (stats?.current_tps ?? 0) * 0.9, stats?.current_tps ?? 0]} />
            </div>
          </div>
          <div className={styles.statCard}>
            <h3>Mempool</h3>
            <div className={styles.value}>{mempoolStats?.total_transactions ?? 0}</div>
            <div className={styles.label}>Pending {mempoolStats?.pending_count ?? 0}</div>
            <div style={{ marginTop: 8 }}>
              <Sparkline data={[mempoolStats?.pending_count ?? 0, (mempoolStats?.pending_count ?? 0) * 0.8 + 1, (mempoolStats?.pending_count ?? 0) * 1.2 + 2, mempoolStats?.pending_count ?? 0]} color="var(--accent-2)" />
            </div>
          </div>
          <div className={styles.statCard}>
            <h3>Synchronization</h3>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <DonutProgress percent={syncProgress.percentage} label="Synced" sublabel={syncProgress.percentage >= 100 ? 'Fully synchronized' : `${syncProgress.synced.toLocaleString()} / ${syncProgress.total.toLocaleString()} blocks`} />
              <div className={styles.progressStats}>
                <div>Head: {stats?.latest_block_height?.toLocaleString?.() ?? '—'}</div>
                <div>Indexed: {stats?.total_blocks?.toLocaleString?.() ?? '—'}</div>
              </div>
            </div>
          </div>
        </div>

        {/* Removed wide sync section; donut moved into top grid */}

        {/* Two-column Recent Blocks / Transactions above search */}
        <div className={styles.statsGrid}>
          <div className={styles.blocksSection}>
            <h2>Recent Blocks</h2>
            {isLoading ? (
              <div className={styles.loading}>Loading block data…</div>
            ) : (
              <div className={styles.blocksContent}>
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
                    {blocks.map((block) => (
                      <tr key={block.height}>
                        <td><a className={styles.hashButton} href={`/blocks/${block.height}`}>{block.height}</a></td>
                        <td><a className={styles.hashButton} href={`/blocks/${block.hash}`}>{middleEllipsis(block.hash, 8)}</a></td>
                        <td>{formatTimestamp(block.timestamp)}</td>
                        <td>{block.transaction_count}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
          <div className={styles.transactionsSection}>
            <h2>Recent Transactions</h2>
            <div className={styles.transactionsContent}>
              <table className={styles.transactionsTable}>
                <thead>
                  <tr>
                    <th>Transaction ID</th>
                    <th>Block</th>
                    <th>Status</th>
                    <th>Created</th>
                  </tr>
                </thead>
                <tbody>
                  {transactions.map((tx) => (
                    <tr key={tx.txid}>
                      <td><a className={styles.hashButton} href={`/tx/${tx.txid}`}>{middleEllipsis(tx.txid, 8)}</a></td>
                      <td><a className={styles.hashButton} href={`/blocks/${tx.block_height}`}>{tx.block_height}</a></td>
                      <td>{formatTransactionStatus(tx.status)}</td>
                      <td>{formatTimestamp(tx.created_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>

        {/* moved search to header; results remain hidden for simplicity */}
      
      {isTxDrawerOpen && (
        <>
          <div className={styles.drawerOverlay} onClick={() => setIsTxDrawerOpen(false)} />
          <aside className={`${styles.drawer} ${styles.drawerOpen}`} role="dialog" aria-modal="true">
            <div className={styles.drawerHeader}>
              <h3 className={styles.drawerTitle}>Transaction Detail</h3>
              <div style={{ display: 'flex', gap: 8 }}>
                <button className={styles.refreshButton} onClick={() => setShowRawJson(v => !v)}>
                  {showRawJson ? 'Hide JSON' : 'Show JSON'}
                </button>
                <button className={styles.refreshButton} onClick={() => setIsTxDrawerOpen(false)}>Close</button>
              </div>
            </div>
            <div className={styles.drawerBody}>
              {drawerTx && (
                <>
                  <div className={styles.blockDetails}>
                    <div className={styles.detailRow}><strong>TXID</strong> <span className={styles.hashValue}>{drawerTx.txid}</span></div>
                    <div className={styles.detailRow}><strong>Block</strong> {drawerTx.block_height}</div>
                    <div className={styles.detailRow}><strong>Status</strong> {formatTransactionStatus(drawerTx.status)}</div>
                    <div className={styles.detailRow}><strong>Created</strong> {formatTimestamp(drawerTx.created_at)}</div>
                  </div>

                  {!showRawJson && (
                    <div>
                      <h4 className={styles.stepHeader} style={{ marginTop: 12 }}>Program Steps</h4>
                      {decodeProgramSteps(drawerTx).length === 0 && (
                        <div className={styles.detailRow}>No decoded steps available.</div>
                      )}
                      {decodeProgramSteps(drawerTx).map((s) => (
                        <div key={s.index} className={styles.step}>
                          <div className={styles.stepHeader}>
                            <span>{s.opcode}</span>
                            <span className={styles.stepMeta}>{s.meta}</span>
                          </div>
                          <div className={styles.stepMeta}>Program: <span className={styles.hashValue}>{s.programId}</span></div>
                        </div>
                      ))}
                    </div>
                  )}

                  {showRawJson && (
                    <pre className={styles.rawJson}>{JSON.stringify((drawerTx as any).data ?? {}, null, 2)}</pre>
                  )}
                </>
              )}
            </div>
            <div className={styles.drawerFooter}>
              <small className={styles.stepMeta}>Tip: Press D to open when a transaction row is selected. Esc to close.</small>
            </div>
          </aside>
        </>
      )}

      {/* Block Modal */}
      {selectedBlock && (
        <div className={styles.modal} onClick={() => setSelectedBlock(null)}>
          <div className={styles.modalContent} onClick={(e) => e.stopPropagation()}>
            <h2>Block Details</h2>
            <p><strong>Height:</strong> {selectedBlock.height}</p>
            <p><strong>Hash:</strong> {selectedBlock.hash}</p>
            <p><strong>Timestamp:</strong> {formatTimestamp(selectedBlock.timestamp)}</p>
            <p><strong>Transactions:</strong> {selectedBlock.transaction_count}</p>
            <button onClick={() => setSelectedBlock(null)}>Close</button>
          </div>
        </div>
      )}

      {/* Transaction Modal removed in favor of drawer-only UX */}
    </Layout>
  );
}
