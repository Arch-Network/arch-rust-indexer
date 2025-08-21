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
  }, []);

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
    if (!timestamp) return 'Unknown';
    try {
      let date;
      if (typeof timestamp === 'string') {
        // Handle PostgreSQL overflow timestamps
        if (timestamp.includes('+') && timestamp.includes('-')) {
          const match = timestamp.match(/\+(\d+)-(\d+)-(\d+)T(\d+):(\d+):(\d+)Z/);
          if (match) {
            const [, year] = match;
            if (parseInt(year) > 9999) {
              return 'Invalid Date (Overflow)';
            }
          }
        }
        date = new Date(timestamp);
      }
      
      if (isNaN(date.getTime())) {
        return 'Invalid Date';
      }
      
      return date.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: true
      });
    } catch (error) {
      return 'Format Error';
    }
  };

  const formatTransactionStatus = (status: any) => {
    if (!status) return 'Unknown';
    try {
      if (typeof status === 'string') {
        return status;
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
              ❌ {errorMsg}
            </span>
          );
        }
        if (status.Success) {
          return (
            <span className={styles.statusSuccess}>
              ✅ Success
            </span>
          );
        }
        if (status.Pending) {
          return (
            <span className={styles.statusPending}>
              ⏳ Pending
            </span>
          );
        }
        return (
          <span className={styles.statusOther}>
            ℹ️ {JSON.stringify(status)}
          </span>
        );
      }
      return 'Unknown';
    } catch (error) {
      return 'Format Error';
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
      <Head>
        <title>Arch Indexer Dashboard</title>
        <meta name="description" content="Blockchain indexer dashboard" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <main className={styles.main}>
        <div className={styles.header}>
          <h1>🚀 Arch Indexer Dashboard</h1>
          <button className={styles.refreshButton} onClick={() => { loadNetworkStats(); loadBlocks(); loadTransactions(); }}>
            🔄 Refresh
          </button>
        </div>

        <div className={styles.statsGrid}>
          <div className={styles.statCard}>
            <h3>Total Blocks</h3>
            <div className={styles.value}>{stats?.total_blocks?.toLocaleString() || '-'}</div>
            <div className={styles.label}>Processed</div>
          </div>
          <div className={styles.statCard}>
            <h3>Total Transactions</h3>
            <div className={styles.value}>{stats?.total_transactions?.toLocaleString() || '-'}</div>
            <div className={styles.label}>Indexed</div>
          </div>
          <div className={styles.statCard}>
            <h3>Latest Block</h3>
            <div className={styles.value}>{stats?.latest_block_height || '-'}</div>
            <div className={styles.label}>Height</div>
          </div>
          <div className={styles.statCard}>
            <h3>Sync Status</h3>
            <div className={styles.value}>🟢 Synced</div>
            <div className={styles.label}>Real-time</div>
          </div>
        </div>

        {/* Sync Progress Section */}
        <div className={styles.syncProgressSection}>
          <div className={styles.syncProgressHeader}>
            <h3>🔄 Blockchain Sync Progress</h3>
            <div className={styles.syncInfo}>
              <span className={styles.syncPercentage}>{syncProgress.percentage}%</span>
              <span className={styles.syncDetails}>
                {syncProgress.percentage >= 100 ? 'Fully Synced' : 
                 syncProgress.percentage >= 90 ? 'Nearly Complete' :
                 syncProgress.percentage >= 50 ? 'Halfway There' : 'Syncing...'}
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
          <h2>🔍 Search Blockchain</h2>
          <div className={styles.searchContainer}>
            <input
              type="text"
              className={styles.searchInput}
              placeholder="Search by block height, block hash, or transaction ID..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyPress={(e) => e.key === 'Enter' && performSearch()}
            />
            <button className={styles.searchButton} onClick={performSearch}>
              Search
            </button>
          </div>
          <div className={styles.searchTips}>
            <small>💡 Tips: Enter a block height (e.g., 52100), block hash, or transaction ID</small>
          </div>
          {searchResults.length > 0 && (
            <div className={styles.searchResults}>
              <h3>Search Results:</h3>
              {searchResults.map((result, index) => (
                <div key={index} className={styles.searchResult}>
                  {result.type === 'block' ? (
                    <div>Block {result.height}: {result.hash}</div>
                  ) : (
                    <div>Transaction: {result.txid}</div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>

        <div className={styles.blocksSection}>
          <h2>📦 Recent Blocks</h2>
          {isLoading ? (
            <div className={styles.loading}>Loading blocks...</div>
          ) : (
            <div className={styles.blocksContent}>
              <table className={styles.blocksTable}>
                <thead>
                  <tr>
                    <th>Height</th>
                    <th>Hash</th>
                    <th>Timestamp</th>
                    <th>Transactions</th>
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
          <h2>💸 Recent Transactions</h2>
          <div className={styles.transactionsContent}>
            <table className={styles.transactionsTable}>
              <thead>
                <tr>
                  <th>Transaction ID</th>
                  <th>Block Height</th>
                  <th>Status</th>
                  <th>Created At</th>
                </tr>
              </thead>
              <tbody>
                {transactions.map((tx) => (
                  <tr key={tx.txid}>
                    <td>
                      <button 
                        className={styles.txidButton}
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
