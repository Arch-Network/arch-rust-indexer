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

  // Get API URL from environment or fallback to localhost
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';

  useEffect(() => {
    loadNetworkStats();
    loadBlocks();
    loadTransactions();
    connectWebSocket();
    createMatrixRain();
  }, []);

  const createMatrixRain = () => {
    const matrixBg = document.createElement('div');
    matrixBg.className = 'matrix-bg';
    
    for (let i = 0; i < 50; i++) {
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
      setSearchResults(data && data.data ? [{ type: data.type, ...data.data }] : []);
    } catch (error) {
      console.error('Search failed:', error);
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
        <title>ARCH INDEXER - BLOCKCHAIN DATA PROCESSOR</title>
        <meta name="description" content="High-performance blockchain indexer and data processor" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <main className={styles.main}>
        <div className={styles.header}>
          <h1 className="glitch">🚀 ARCH INDEXER - BLOCKCHAIN DATA PROCESSOR</h1>
          <button className={styles.refreshButton} onClick={() => { loadNetworkStats(); loadBlocks(); loadTransactions(); }}>
            [REFRESH DATA]
          </button>
        </div>

        <div className={styles.statsGrid}>
          <div className={styles.statCard}>
            <h3>BLOCKS PROCESSED</h3>
            <div className={styles.value}>{stats?.total_blocks?.toLocaleString() || '0'}</div>
            <div className={styles.label}>TOTAL BLOCKS INDEXED</div>
          </div>
          <div className={styles.statCard}>
            <h3>TRANSACTIONS INDEXED</h3>
            <div className={styles.value}>{stats?.total_transactions?.toLocaleString() || '0'}</div>
            <div className={styles.label}>TOTAL TX PROCESSED</div>
          </div>
          <div className={styles.statCard}>
            <h3>LATEST BLOCK HEIGHT</h3>
            <div className={styles.value}>{stats?.latest_block_height || '0'}</div>
            <div className={styles.label}>CURRENT CHAIN HEAD</div>
          </div>
          <div className={styles.statCard}>
            <h3>SYNC STATUS</h3>
            <div className={styles.value}>🟢 [SYNCED]</div>
            <div className={styles.label}>REAL-TIME INDEXING</div>
          </div>
        </div>

        {/* Sync Progress Section */}
        <div className={styles.syncProgressSection}>
          <div className={styles.syncProgressHeader}>
            <h3>🔄 BLOCKCHAIN SYNCHRONIZATION PROGRESS</h3>
            <div className={styles.syncInfo}>
              <span className={styles.syncPercentage}>{syncProgress.percentage}%</span>
              <span className={styles.syncDetails}>
                {syncProgress.percentage >= 100 ? 'FULLY SYNCHRONIZED' : 
                 syncProgress.percentage >= 90 ? 'NEARLY COMPLETE' :
                 syncProgress.percentage >= 50 ? 'HALFWAY COMPLETE' : 'SYNCING...'}
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
              <span>{syncProgress.synced.toLocaleString()}</span> / <span>{syncProgress.total.toLocaleString()}</span> BLOCKS
            </div>
          </div>
        </div>

        <div className={styles.searchSection}>
          <h2>🔍 BLOCKCHAIN DATA QUERY INTERFACE</h2>
          <div className={styles.searchContainer}>
            <input
              type="text"
              className={styles.searchInput}
              placeholder="ENTER BLOCK HEIGHT, HASH, OR TRANSACTION ID..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyPress={(e) => e.key === 'Enter' && performSearch()}
            />
            <button className={styles.searchButton} onClick={performSearch}>
              [EXECUTE QUERY]
            </button>
          </div>
          <div className={styles.searchTips}>
            <small>💡 QUERY FORMAT: BLOCK HEIGHT (e.g., 52100), BLOCK HASH, OR TRANSACTION ID</small>
          </div>
          {searchResults.length > 0 && (
            <div className={styles.searchResults}>
              <h3>QUERY RESULTS:</h3>
              {searchResults.map((result, index) => (
                <div key={index} className={styles.searchResult}>
                  {result.type === 'block' ? (
                    <div>BLOCK {result.height}: {result.hash}</div>
                  ) : (
                    <div>TRANSACTION: {result.txid}</div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>

        <div className={styles.blocksSection}>
          <h2>📦 RECENT BLOCK DATA</h2>
          {isLoading ? (
            <div className={styles.loading}>[LOADING BLOCK DATA...]</div>
          ) : (
            <div className={styles.blocksContent}>
              <table className={styles.blocksTable}>
                <thead>
                  <tr>
                    <th>BLOCK HEIGHT</th>
                    <th>BLOCK HASH</th>
                    <th>TIMESTAMP</th>
                    <th>TX COUNT</th>
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
          <h2>💸 RECENT TRANSACTION DATA</h2>
          <div className={styles.transactionsContent}>
            <table className={styles.transactionsTable}>
              <thead>
                <tr>
                  <th>TRANSACTION ID</th>
                  <th>BLOCK HEIGHT</th>
                  <th>EXECUTION STATUS</th>
                  <th>CREATION TIMESTAMP</th>
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
            <h2>BLOCK DATA DETAILS</h2>
            <p><strong>HEIGHT:</strong> {selectedBlock.height}</p>
            <p><strong>HASH:</strong> {selectedBlock.hash}</p>
            <p><strong>TIMESTAMP:</strong> {formatTimestamp(selectedBlock.timestamp)}</p>
            <p><strong>TRANSACTIONS:</strong> {selectedBlock.transaction_count}</p>
            <button onClick={() => setSelectedBlock(null)}>[CLOSE]</button>
          </div>
        </div>
      )}

      {/* Transaction Modal */}
      {selectedTransaction && (
        <div className={styles.modal} onClick={() => setSelectedTransaction(null)}>
          <div className={styles.modalContent} onClick={(e) => e.stopPropagation()}>
            <h2>TRANSACTION DATA DETAILS</h2>
            <p><strong>ID:</strong> {selectedTransaction.txid}</p>
            <p><strong>BLOCK HEIGHT:</strong> {selectedTransaction.block_height}</p>
            <p><strong>STATUS:</strong> {formatTransactionStatus(selectedTransaction.status)}</p>
            <p><strong>CREATED:</strong> {formatTimestamp(selectedTransaction.created_at)}</p>
            <button onClick={() => setSelectedTransaction(null)}>[CLOSE]</button>
          </div>
        </div>
      )}
    </div>
  );
}
