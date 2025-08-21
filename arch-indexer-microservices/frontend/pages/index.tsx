import React, { useState, useEffect } from 'react';
import Head from 'next/head';
import styles from '../styles/Home.module.css';

interface NetworkStats {
  total_blocks: number;
  total_transactions: number;
  latest_block_height: number;
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

  // Get API URL from environment or fallback to localhost
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';

  useEffect(() => {
    loadNetworkStats();
    loadBlocks();
    loadTransactions();
    connectWebSocket();
    createMatrixRain();
  }, []);

  useEffect(() => {
    // Toggle meme mode by switching a class on html element
    const html = document.querySelector('html');
    if (!html) return;
    if (memeMode) {
      html.classList.add('meme-mode');
    } else {
      html.classList.remove('meme-mode');
    }
  }, [memeMode]);

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
              <strong>TRANSACTION COUNT:</strong> {result.data.transaction_count || '0'}
            </div>
            <div className={styles.detailRow}>
              <strong>BLOCK SIZE:</strong> {result.data.size ? `${result.data.size} bytes` : 'UNKNOWN'}
            </div>
            <div className={styles.detailRow}>
              <strong>PREVIOUS BLOCK:</strong> 
              {result.data.previous_block_hash ? (
                <span className={styles.hashValue}>{result.data.previous_block_hash.substring(0, 16)}...</span>
              ) : 'GENESIS BLOCK'}
            </div>
            <div className={styles.detailRow}>
              <strong>MERKLE ROOT:</strong> 
              {result.data.merkle_root ? (
                <span className={styles.hashValue}>{result.data.merkle_root}</span>
              ) : 'UNKNOWN'}
            </div>
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
      
      return date.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: true
      }).toUpperCase();
    } catch (error) {
      return 'FORMAT ERROR';
    }
  };

  const formatTransactionStatus = (status: any) => {
    if (!status) return 'UNKNOWN';
    try {
      if (typeof status === 'string') {
        return status.toUpperCase();
      } else if (typeof status === 'object') {
        if (status.Failed) {
          let errorMsg = status.Failed;
          if (errorMsg.includes('pubkey:') && errorMsg.includes('[')) {
            const pubkeyIndex = errorMsg.indexOf('pubkey:');
            if (pubkeyIndex > 0) {
              errorMsg = errorMsg.substring(0, pubkeyIndex).trim();
            }
          }
          if (errorMsg.length > 60) {
            errorMsg = errorMsg.substring(0, 60) + '...';
          }
          return (
            <span className={styles.statusFailed} title={status.Failed}>
              [FAILED] {errorMsg}
            </span>
          );
        }
        if (status.Success) {
          return (
            <span className={styles.statusSuccess}>
              [SUCCESS] EXECUTED
            </span>
          );
        }
        if (status.Pending) {
          return (
            <span className={styles.statusPending}>
              [PENDING] QUEUED
            </span>
          );
        }
        return (
          <span className={styles.statusOther}>
            [INFO] {JSON.stringify(status)}
          </span>
        );
      }
      return 'UNKNOWN';
    } catch (error) {
      return 'FORMAT ERROR';
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
    <div className={styles.container}>
      {/* CRT Scanline Effect */}
      <div className="scanline"></div>
      
      <Head>
        <title>Arch Explorer • Cypherpunk Mode</title>
        <meta name="description" content="Fresh, usable blockchain explorer for Arch" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <main className={styles.main}>
        <div className={styles.header}>
          <h1>Arch Explorer</h1>
          <div style={{ display: 'flex', gap: 12 }}>
            <button className={styles.refreshButton} onClick={() => { loadNetworkStats(); loadBlocks(); loadTransactions(); }}>
              Refresh
            </button>
            <button className={styles.refreshButton} onClick={() => setMemeMode(v => !v)}>
              {memeMode ? 'Meme Mode: ON' : 'Meme Mode: OFF'}
            </button>
          </div>
        </div>

        <div className={styles.statsGrid}>
          <div className={styles.statCard}>
            <h3>Blocks Indexed</h3>
            <div className={styles.value}>{stats?.total_blocks?.toLocaleString() || '0'}</div>
            <div className={styles.label}>Total Blocks</div>
          </div>
          <div className={styles.statCard}>
            <h3>Transactions</h3>
            <div className={styles.value}>{stats?.total_transactions?.toLocaleString() || '0'}</div>
            <div className={styles.label}>Indexed</div>
          </div>
          <div className={styles.statCard}>
            <h3>Chain Head</h3>
            <div className={styles.value}>{stats?.latest_block_height || '0'}</div>
            <div className={styles.label}>Latest Block</div>
          </div>
          <div className={styles.statCard}>
            <h3>Status</h3>
            <div className={styles.value}>[SYNCED]</div>
            <div className={styles.label}>Realtime</div>
          </div>
        </div>

        {/* Sync Progress Section */}
        <div className={styles.syncProgressSection}>
          <div className={styles.syncProgressHeader}>
            <h3>Synchronization Progress</h3>
            <div className={styles.syncInfo}>
              <span className={styles.syncPercentage}>{syncProgress.percentage}%</span>
              <span className={styles.syncDetails}>
                {syncProgress.percentage >= 100 ? 'Fully Synchronized' : 
                 syncProgress.percentage >= 90 ? 'Nearly Complete' :
                 syncProgress.percentage >= 50 ? 'Halfway Complete' : 'Syncing...'}
              </span>
            </div>
          </div>
          <div className={styles.progressBarContainer}>
            <div className={styles.progressBar}>
              <div 
                className={styles.progressFill} 
                style={{ width: `${syncProgress.percentage}%` }}
              />
            </div>
            <div className={styles.progressStats}>
              <span>{syncProgress.synced.toLocaleString()}</span> / <span>{syncProgress.total.toLocaleString()}</span> blocks
            </div>
          </div>
        </div>

        <div className={styles.searchSection}>
          <h2>Search Blockchain</h2>
          <div className={styles.searchContainer}>
            <input
              type="text"
              className={styles.searchInput}
              placeholder="Enter block height, block hash, or transaction ID..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyPress={(e) => e.key === 'Enter' && performSearch()}
            />
            <button className={styles.searchButton} onClick={performSearch}>
              Execute
            </button>
          </div>
          <div className={styles.searchTips}>
            <small>Tips: Height (e.g., 52100), block hash, or transaction ID</small>
          </div>
          {searchResults.length > 0 && (
            <div className={styles.searchResults}>
              <h3>Query Results</h3>
              {searchResults.map((result, index) => (
                <div key={index}>{renderSearchResult(result)}</div>
              ))}
            </div>
          )}
        </div>

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
                      <td>{block.height}</td>
                      <td>
                        <button 
                          className={styles.hashButton}
                          onClick={() => setSelectedBlock(block)}
                        >
                          {block.hash.substring(0, 16)}...
                        </button>
                      </td>
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
                    <td>
                      <button 
                        className={styles.hashButton}
                        onClick={() => setSelectedTransaction(tx)}
                      >
                        {tx.txid.substring(0, 16)}...
                      </button>
                    </td>
                    <td>{tx.block_height}</td>
                    <td>{formatTransactionStatus(tx.status)}</td>
                    <td>{formatTimestamp(tx.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </main>

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

      {/* Transaction Modal */}
      {selectedTransaction && (
        <div className={styles.modal} onClick={() => setSelectedTransaction(null)}>
          <div className={styles.modalContent} onClick={(e) => e.stopPropagation()}>
            <h2>Transaction Details</h2>
            <p><strong>ID:</strong> {selectedTransaction.txid}</p>
            <p><strong>Block Height:</strong> {selectedTransaction.block_height}</p>
            <p><strong>Status:</strong> {formatTransactionStatus(selectedTransaction.status)}</p>
            <p><strong>Created:</strong> {formatTimestamp(selectedTransaction.created_at)}</p>
            <button onClick={() => setSelectedTransaction(null)}>Close</button>
          </div>
        </div>
      )}
    </div>
  );
}
