import React, { useState, useEffect, useRef } from 'react';
import Head from 'next/head';
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
import styles from '../styles/Home.module.css';

// Show start and end of long identifiers like hashes
const middleEllipsis = (value: string, keep: number = 8): string => {
  if (!value || typeof value !== 'string') return '';
  if (value.length <= keep * 2 + 1) return value;
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
};

interface Block {
  height: number;
  hash: string;
  timestamp: string;
  transactions: number;
  size: number;
}

interface Transaction {
  signature: string;
  block_height: number;
  timestamp: string;
  fee: number;
  status: string;
}

interface Stats {
  total_blocks: number;
  total_transactions: number;
  latest_block: number;
  total_size: number;
}

export default function Home() {
  const [stats, setStats] = useState<Stats>({
    total_blocks: 0,
    total_transactions: 0,
    latest_block: 0,
    total_size: 0
  });
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearchResults, setShowSearchResults] = useState(false);
  const [currentPage, setCurrentPage] = useState(0);
  const [loading, setLoading] = useState(false);
  const [websocketStatus, setWebsocketStatus] = useState('📡 Connecting...');
  const [selectedBlock, setSelectedBlock] = useState<Block | null>(null);
  const [selectedTransaction, setSelectedTransaction] = useState<Transaction | null>(null);
  const [showBlockModal, setShowBlockModal] = useState(false);
  const [showTransactionModal, setShowTransactionModal] = useState(false);
  
  const websocketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    loadStats();
    loadBlocks(0);
    loadTransactions(0);
    connectWebSocket();
    
    return () => {
      if (websocketRef.current) {
        websocketRef.current.close();
      }
    };
  }, []);

  const connectWebSocket = () => {
    // Connect to the indexer's WebSocket server
    const wsUrl = `ws://localhost:9090/ws`;
    const ws = new WebSocket(wsUrl);
    websocketRef.current = ws;

    ws.onopen = () => {
      setWebsocketStatus('📡 Connected');
      
      // Subscribe to block events
      const subscribeMsg = {
        method: 'subscribe',
        params: {
          topic: 'block',
          filter: {},
          request_id: 'explorer_' + Date.now()
        }
      };
      ws.send(JSON.stringify(subscribeMsg));
    };

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        
        if (data.topic === 'block') {
          updateRealtimeStats(data.data);
        }
      } catch (error) {
        console.error('Failed to parse WebSocket message:', error);
      }
    };

    ws.onclose = () => {
      setWebsocketStatus('📡 Disconnected');
      
      // Try to reconnect after 5 seconds
      setTimeout(connectWebSocket, 5000);
    };

    ws.onerror = (error) => {
      console.error('WebSocket error:', error);
      setWebsocketStatus('📡 Error');
    };
  };

  const updateRealtimeStats = (blockData: any) => {
    if (blockData.height) {
      setStats(prev => ({
        ...prev,
        latest_block: blockData.height,
        total_blocks: prev.total_blocks + 1
      }));
    }
    
    // Refresh blocks list if we're on the first page
    if (currentPage === 0) {
      loadBlocks(0);
    }
  };

  const loadStats = async () => {
    try {
      const response = await fetch('/api/stats');
      if (response.ok) {
        const data = await response.json();
        setStats(data);
      }
    } catch (error) {
      console.error('Failed to load stats:', error);
    }
  };

  const loadBlocks = async (page: number) => {
    try {
      setLoading(true);
      const response = await fetch(`/api/blocks?page=${page}&limit=20`);
      if (response.ok) {
        const data = await response.json();
        setBlocks(data.blocks || []);
        setCurrentPage(page);
      }
    } catch (error) {
      console.error('Failed to load blocks:', error);
    } finally {
      setLoading(false);
    }
  };

  const loadTransactions = async (page: number) => {
    try {
      const response = await fetch(`/api/transactions?page=${page}&limit=20`);
      if (response.ok) {
        const data = await response.json();
        setTransactions(data.transactions || []);
      }
    } catch (error) {
      console.error('Failed to load transactions:', error);
    }
  };

  const performSearch = async () => {
    if (!searchQuery.trim()) return;
    
    try {
      setLoading(true);
      const response = await fetch(`/api/search?q=${encodeURIComponent(searchQuery)}`);
      if (response.ok) {
        const data = await response.json();
        setSearchResults(data.results || []);
        setShowSearchResults(true);
      }
    } catch (error) {
      console.error('Search failed:', error);
    } finally {
      setLoading(false);
    }
  };

  const handleSearchKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      performSearch();
    }
  };

  const openBlockModal = (block: Block) => {
    setSelectedBlock(block);
    setShowBlockModal(true);
  };

  const openTransactionModal = (tx: Transaction) => {
    setSelectedTransaction(tx);
    setShowTransactionModal(true);
  };

  const closeModal = () => {
    setShowBlockModal(false);
    setShowTransactionModal(false);
    setSelectedBlock(null);
    setSelectedTransaction(null);
  };

  return (
    <div className={styles.container}>
      <Head>
        <title>Arch Block Explorer</title>
        <meta name="description" content="Real-time Arch blockchain explorer" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <div className={styles.header}>
        <h1>Arch Block Explorer</h1>
        <p>Real-time blockchain data and analytics</p>
      </div>

      <div className={styles.statsHeader}>
        <h2>Network Statistics</h2>
        <div className={styles.websocketStatus} style={{ background: websocketStatus.includes('Connected') ? '#28a745' : '#dc3545' }}>
          {websocketStatus}
        </div>
        <button className={styles.refreshButton} onClick={loadStats}>
          🔄 Refresh
        </button>
      </div>

      <div className={styles.statsGrid}>
        <div className={styles.statCard}>
          <h3>Total Blocks</h3>
          <div className={styles.value}>{stats.total_blocks.toLocaleString()}</div>
          <div className={styles.label}>All time</div>
        </div>
        <div className={styles.statCard}>
          <h3>Total Transactions</h3>
          <div className={styles.value}>{stats.total_transactions.toLocaleString()}</div>
          <div className={styles.label}>All time</div>
        </div>
        <div className={styles.statCard}>
          <h3>Latest Block</h3>
          <div className={styles.value} id="latest-block">{stats.latest_block.toLocaleString()}</div>
          <div className={styles.label}>Current height</div>
        </div>
        <div className={styles.statCard}>
          <h3>Total Size</h3>
          <div className={styles.value}>{(stats.total_size / 1024 / 1024).toFixed(2)} MB</div>
          <div className={styles.label}>Blockchain size</div>
        </div>
      </div>

      <div className={styles.searchSection}>
        <h2>Search Blockchain</h2>
        <div className={styles.searchContainer}>
          <input
            type="text"
            className={styles.searchInput}
            placeholder="Search by block height, transaction signature, or address..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyPress={handleSearchKeyPress}
          />
        </div>
        <button className={styles.searchButton} onClick={performSearch} disabled={loading}>
          {loading ? '🔍 Searching...' : '🔍 Search'}
        </button>
        <div className={styles.searchTips}>
          <p>💡 Tip: You can search for block heights, transaction signatures, or wallet addresses</p>
        </div>
        
        {showSearchResults && (
          <div className={`${styles.searchResults} ${styles.show}`}>
            <h3>Search Results</h3>
            {searchResults.length > 0 ? (
              searchResults.map((result, index) => (
                <div key={index} className={styles.searchResultItem}>
                  <h4>{result.type}: {result.value}</h4>
                  <div className={styles.details}>{result.description}</div>
                </div>
              ))
            ) : (
              <p>No results found for "{searchQuery}"</p>
            )}
          </div>
        )}
      </div>

      <div className={styles.blocksSection}>
        <h2>Recent Blocks</h2>
        <div className={styles.tableContainer}>
          <table className={styles.dataTable}>
            <thead>
              <tr>
                <th>Height</th>
                <th>Hash</th>
                <th>Timestamp</th>
                <th>Transactions</th>
                <th>Size</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {blocks.map((block) => (
                <tr key={block.height}>
                  <td>{block.height.toLocaleString()}</td>
                  <td className={styles.hashCell}>{middleEllipsis(block.hash, 8)}</td>
                  <td>{new Date(block.timestamp).toLocaleString()}</td>
                  <td>{block.transactions}</td>
                  <td>{(block.size / 1024).toFixed(2)} KB</td>
                  <td>
                    <button 
                      className={styles.viewButton}
                      onClick={() => openBlockModal(block)}
                    >
                      👁️ View
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        
        <div className={styles.pagination}>
          <button 
            className={styles.pageButton} 
            onClick={() => loadBlocks(currentPage - 1)}
            disabled={currentPage === 0}
          >
            ← Previous
          </button>
          <span className={styles.pageInfo}>Page {currentPage + 1}</span>
          <button 
            className={styles.pageButton} 
            onClick={() => loadBlocks(currentPage + 1)}
            disabled={blocks.length < 20}
          >
            Next →
          </button>
        </div>
      </div>

      <div className={styles.transactionsSection}>
        <h2>Recent Transactions</h2>
        <div className={styles.tableContainer}>
          <table className={styles.dataTable}>
            <thead>
              <tr>
                <th>Signature</th>
                <th>Block Height</th>
                <th>Timestamp</th>
                <th>Fee</th>
                <th>Status</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {transactions.map((tx) => (
                <tr key={tx.signature}>
                  <td className={styles.hashCell}>{middleEllipsis(tx.signature, 8)}</td>
                  <td>{tx.block_height.toLocaleString()}</td>
                  <td>{new Date(tx.timestamp).toLocaleString()}</td>
                  <td>{tx.fee} ARCH</td>
                  <td>
                    <span className={`${styles.status} ${styles[tx.status.toLowerCase()]}`}>
                      {tx.status}
                    </span>
                  </td>
                  <td>
                    <button 
                      className={styles.viewButton}
                      onClick={() => openTransactionModal(tx)}
                    >
                      👁️ View
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Block Modal */}
      {showBlockModal && selectedBlock && (
        <div className={styles.modal} id="block-modal">
          <div className={styles.modalContent}>
            <span className={styles.close} onClick={closeModal}>&times;</span>
            <h2>Block Details</h2>
            <div className={styles.modalDetails}>
              <p><strong>Height:</strong> {selectedBlock.height.toLocaleString()}</p>
              <p><strong>Hash:</strong> {selectedBlock.hash}</p>
              <p><strong>Timestamp:</strong> {new Date(selectedBlock.timestamp).toLocaleString()}</p>
              <p><strong>Transactions:</strong> {selectedBlock.transactions}</p>
              <p><strong>Size:</strong> {(selectedBlock.size / 1024).toFixed(2)} KB</p>
            </div>
          </div>
        </div>
      )}

      {/* Transaction Modal */}
      {showTransactionModal && selectedTransaction && (
        <div className={styles.modal} id="transaction-modal">
          <div className={styles.modalContent}>
            <span className={styles.close} onClick={closeModal}>&times;</span>
            <h2>Transaction Details</h2>
            <div className={styles.modalDetails}>
              <p><strong>Signature:</strong> {selectedTransaction.signature}</p>
              <p><strong>Block Height:</strong> {selectedTransaction.block_height.toLocaleString()}</p>
              <p><strong>Timestamp:</strong> {new Date(selectedTransaction.timestamp).toLocaleString()}</p>
              <p><strong>Fee:</strong> {selectedTransaction.fee} ARCH</p>
              <p><strong>Status:</strong> {selectedTransaction.status}</p>
            </div>
          </div>
        </div>
      )}

      <div className={styles.footer}>
        <p>&copy; 2024 Arch Block Explorer. Built with Next.js and real-time WebSocket updates.</p>
      </div>
    </div>
  );
}
