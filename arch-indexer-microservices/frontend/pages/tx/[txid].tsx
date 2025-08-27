import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/router';
import Link from 'next/link';
import Layout from '../../components/Layout';
import styles from '../../styles/Home.module.css';
import dynamic from 'next/dynamic';
import Button from '../../components/Button';
const JsonViewer = dynamic(() => import('../../components/JsonViewer'), { ssr: false });

type Tx = { txid: string; block_height: number; status?: any; created_at: string; data?: any };
type Participant = { address_hex: string; address_base58: string; is_signer: boolean; is_writable: boolean; is_readonly: boolean; is_fee_payer: boolean };
type Instruction = { index: number; program_id_hex: string; program_id_base58: string; accounts: string[]; data_len: number; action?: string | null; decoded?: any; data_hex?: string };
type Execution = { status: any; logs: string[]; bitcoin_txid?: string | null; rollback_status?: any; compute_units_consumed?: number | null; runtime_transaction?: any };

export default function TxDetailPage() {
  const router = useRouter();
  const txid = router.query.txid as string | undefined;
  const apiUrl = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
  const [tx, setTx] = useState<Tx | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);
  const [copied, setCopied] = useState(false);
  const [participants, setParticipants] = useState<Participant[] | null>(null);
  const [instructions, setInstructions] = useState<Instruction[] | null>(null);
  const [execution, setExecution] = useState<Execution | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);

  useEffect(() => {
    if (!txid) return;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${apiUrl}/api/transactions/${txid}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        setTx(json);
        // Fetch auxiliary sections in parallel (best-effort)
        const [pRes, iRes, eRes] = await Promise.allSettled([
          fetch(`${apiUrl}/api/transactions/${txid}/participants`),
          fetch(`${apiUrl}/api/transactions/${txid}/instructions`),
          fetch(`${apiUrl}/api/transactions/${txid}/execution`),
        ]);
        if (pRes.status === 'fulfilled' && pRes.value.ok) {
          setParticipants(await pRes.value.json());
        } else {
          setParticipants([]);
        }
        if (iRes.status === 'fulfilled' && iRes.value.ok) {
          setInstructions(await iRes.value.json());
        } else {
          setInstructions([]);
        }
        if (eRes.status === 'fulfilled' && eRes.value.ok) {
          setExecution(await eRes.value.json());
        } else {
          setExecution(null);
        }
      } catch (e: any) {
        setError('Transaction not found');
      } finally {
        setLoading(false);
      }
    })();
  }, [txid, apiUrl]);

  const formatStatus = (s: any) => {
    if (!s) return 'PENDING';
    if (typeof s === 'string') {
      const up = s.toUpperCase();
      if (up.includes('PROCESSED') || up.includes('SUCCESS')) return 'SUCCESS';
      if (up.includes('FAIL')) return 'FAILED';
      if (up.includes('PEND')) return 'PENDING';
      return 'INFO';
    }
    const up = JSON.stringify(s).toUpperCase();
    if (up.includes('PROCESSED') || up.includes('SUCCESS')) return 'SUCCESS';
    if (up.includes('FAILED') || up.includes('ERROR')) return 'FAILED';
    if (up.includes('PENDING')) return 'PENDING';
    return 'INFO';
  };

  return (
    <Layout rightActions={<button className={styles.refreshButton} onClick={() => router.reload()}>Refresh</button>}>
      <section className={styles.searchSection}>
        <h2>Transaction Detail</h2>
        {loading && <div className={styles.loading}>Loading…</div>}
        {error && <div className={styles.statusOther}>{error}</div>}
        {tx && (
          <div className={styles.blockDetails}>
            <div className={styles.detailRow}><strong>TxID</strong> <span className={styles.hashValue}>{tx.txid}</span></div>
            <div className={styles.detailRow}><strong>Block</strong> <Link href={`/blocks/${tx.block_height}`} className={styles.hashButton}>{tx.block_height}</Link></div>
            <div className={styles.detailRow}><strong>Status</strong> {formatStatus(tx.status)}</div>
            <div className={styles.detailRow}><strong>Created</strong> {tx.created_at ? new Date(tx.created_at).toLocaleString() : '—'}</div>
            <div className={styles.detailRow}><strong>Actions</strong> <button className={styles.searchButton} onClick={() => setShowRaw(v => !v)}>{showRaw ? 'Hide JSON' : 'Show JSON'}</button></div>
          </div>
        )}
      </section>
      {showRaw && tx?.data && (
        <section className={styles.searchSection}>
          <h2>Raw JSON</h2>
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 8, gap: 8 }}>
            <Button
              size="sm"
              onClick={async () => {
                try {
                  const text = JSON.stringify(tx.data, null, 2);
                  if (navigator?.clipboard?.writeText) {
                    await navigator.clipboard.writeText(text);
                  } else {
                    const ta = document.createElement('textarea');
                    ta.value = text; document.body.appendChild(ta); ta.select(); document.execCommand('copy'); document.body.removeChild(ta);
                  }
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                } catch {}
              }}
            >
              {copied ? 'Copied!' : 'Copy JSON'}
            </Button>
          </div>
          <div className={styles.rawJson}>
            <JsonViewer data={tx.data} initiallyExpanded={true} />
          </div>
        </section>
      )}

      {participants && (
        <section className={styles.searchSection}>
          <h2>Participants</h2>
          {participants.length === 0 ? (
            <div className={styles.muted}>No participants found.</div>
          ) : (
            <div className={styles.blockDetails}>
              {participants.map((p, idx) => (
                <div key={idx} className={styles.detailRow}>
                  <strong style={{ minWidth: 120, display: 'inline-block' }}>Address</strong>
                  <Link href={`/accounts/${p.address_base58}`} className={styles.hashButton}>{p.address_base58}</Link>
                  <span style={{ marginLeft: 12, fontSize: 12 }}>
                    {p.is_fee_payer && <span className={styles.statusInfo} style={{ marginRight: 6 }}>fee payer</span>}
                    {p.is_signer && <span className={styles.statusInfo} style={{ marginRight: 6 }}>signer</span>}
                    {p.is_writable ? (
                      <span className={styles.statusSuccess}>writable</span>
                    ) : (
                      <span className={styles.statusOther}>readonly</span>
                    )}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>
      )}

      {instructions && (
        <section className={styles.searchSection}>
          <h2>Instructions</h2>
          {instructions.length === 0 ? (
            <div className={styles.muted}>No instructions.</div>
          ) : (
            <div className={styles.blockDetails}>
              {instructions.map((ins) => (
                <div key={ins.index} className={styles.instructionCard}>
                  <div style={{display:'flex', flexDirection:'column', width:'100%'}}>
                    <div style={{display:'flex', alignItems:'center', justifyContent:'space-between', flexWrap:'wrap'}}>
                      <div className={styles.instructionMeta}>
                        <span className={styles.badge}>#{ins.index}</span>
                        <Link href={`/programs/${ins.program_id_hex}`} className={styles.hashButton}>
                          {ins.program_id_base58 || ins.program_id_hex || 'Unknown program'}
                        </Link>
                        {ins.action && <span className={styles.statusInfo}>{ins.action}</span>}
                      </div>
                    </div>
                    {/* Visual summary for common actions */}
                    {(() => {
                      try {
                        const d = ins.decoded;
                        if (ins.action?.toLowerCase().includes('transfer') && d?.lamports?.data != null) {
                          const lam = d.lamports.data as number; const sol = lam / 1_000_000_000;
                          const src = d.source || ins.accounts[0];
                          const dst = d.destination || ins.accounts[1];
                          return (
                            <div className={styles.vizRow}>
                              <Link href={`/accounts/${src}`} className={styles.hashButton}>{src}</Link>
                              <span className={styles.arrow}>→</span>
                              <Link href={`/accounts/${dst}`} className={styles.hashButton}>{dst}</Link>
                              <span className={styles.amountPill}>Amount: {sol} SOL</span>
                            </div>
                          );
                        }
                        if (ins.action?.toLowerCase().includes('createaccount')) {
                          const funder = d?.funder || ins.accounts[0];
                          const newAcc = d?.new_account || ins.accounts[1];
                          const space = d?.space?.data ?? 0;
                          const lam = d?.lamports?.data ?? 0; const sol = lam / 1_000_000_000;
                          return (
                            <div className={styles.vizRow}>
                              <Link href={`/accounts/${funder}`} className={styles.hashButton}>{funder}</Link>
                              <span className={styles.arrow}>funds</span>
                              <Link href={`/accounts/${newAcc}`} className={styles.hashButton}>{newAcc}</Link>
                              <span className={styles.amountPill}>{sol} SOL</span>
                              <span className={styles.badge}>space {space} bytes</span>
                            </div>
                          );
                        }
                      } catch {}
                      return null;
                    })()}

                    <div style={{ marginTop: 8 }}>
                      <span className={styles.muted} style={{ marginRight: 6 }}>Accounts:</span>
                      {ins.accounts.map((a, i) => (
                        <Link key={i} href={`/accounts/${a}`} className={styles.hashButton} style={{ marginRight: 6 }}>{a}</Link>
                      ))}
                    </div>
                    {/* Summary line for common decoded fields */}
                    {ins.decoded && (
                      <div style={{ marginTop: 8 }}>
                        {(() => {
                          try {
                            const d = ins.decoded;
                            if (d?.lamports?.data != null) {
                              const lam = d.lamports.data as number;
                              const sol = lam / 1_000_000_000;
                              return <div className={styles.statusOther}>Amount: {sol} SOL</div>;
                            }
                            if (d?.memo) {
                              return <div className={styles.statusOther}>Memo: {String(d.memo)}</div>;
                            }
                            if (d?.units?.data != null) {
                              return <div className={styles.statusOther}>Compute units: {d.units.data}</div>;
                            }
                          } catch {}
                          return null;
                        })()}
                      </div>
                    )}
                    {/* Raw instruction toggle (show even when 0 bytes) */}
                    {(ins.data_hex !== undefined) && (
                      <div style={{ marginTop: 8 }}>
                        <details>
                          <summary className={styles.muted} style={{ cursor: 'pointer' }}>Show raw instruction ({ins.data_len} bytes)</summary>
                          <div style={{ marginTop: 6, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                            <code style={{ overflowWrap: 'anywhere' }}>{ins.data_hex || '—'}</code>
                            <Button size="sm" onClick={async () => { try { await navigator.clipboard.writeText(ins.data_hex || ''); } catch {} }}>Copy</Button>
                          </div>
                        </details>
                      </div>
                    )}

                    {ins.decoded && (
                      <div className={styles.rawJson} style={{ marginTop: 8 }}>
                        <JsonViewer data={ins.decoded} initiallyExpanded={true} />
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      )}

      {execution && (
        <section className={styles.searchSection}>
          <h2>Execution</h2>
          <div className={styles.blockDetails}>
            <div className={styles.detailRow}><strong>Compute Units</strong> {execution.compute_units_consumed ?? '—'}</div>
            <div className={styles.detailRow}><strong>Bitcoin TxID</strong> {execution.bitcoin_txid || '—'}</div>
            <div className={styles.detailRow}>
              <strong>Logs</strong>
              <button className={styles.searchButton} style={{ marginLeft: 12 }} onClick={() => setLogsOpen(v => !v)}>
                {logsOpen ? 'Hide Logs' : 'Show Logs'}
              </button>
            </div>
            {logsOpen && (
              <div className={styles.rawJson} style={{ maxHeight: 320, overflow: 'auto', padding: 12 }}>
                <pre style={{ margin: 0 }}>{(execution.logs || []).join('\n')}</pre>
              </div>
            )}
          </div>
        </section>
      )}
      <div className={styles.searchTips}><Link href="/tx">← Back to Transactions</Link></div>
    </Layout>
  );
}
