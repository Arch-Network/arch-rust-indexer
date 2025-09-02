import React, { useEffect, useMemo, useRef, useState } from 'react';

type ProgramMix = Record<string, number>; // program_id -> share (0..1) or raw counts
type BlockCol = { height: number; ts: number; mix: ProgramMix; txs?: number };

type Props = {
  apiUrl: string;
  height?: number;
  maxCols?: number; // number of recent blocks to show
  maxPrograms?: number; // number of top programs rows
};

function colorForProgram(id: string): string {
  // Hash to pastel-ish color
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  const hue = h % 360;
  return `hsl(${hue} 70% 55%)`;
}

function pick(field: any, keys: string[]): any {
  for (const k of keys) {
    const v = field?.[k];
    if (v != null) return v;
  }
  return undefined;
}

export default function ProgramHeatstrip({ apiUrl, height = 80, maxCols = 200, maxPrograms = 18 }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [cols, setCols] = useState<BlockCol[]>([]);
  const globalCountsRef = useRef<Map<string, number>>(new Map());
  const [topPrograms, setTopPrograms] = useState<string[]>([]); // when empty, we fallback to __TX__ row
  const [useTxFallback, setUseTxFallback] = useState<boolean>(false);
  const [width, setWidth] = useState<number>(1200);
  const devicePixelRatioRef = useRef<number>(1);

  // Resize canvas for DPR and container width
  useEffect(() => {
    const dpr = Math.min(2, typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1);
    devicePixelRatioRef.current = dpr;
    const c = canvasRef.current; const host = containerRef.current; if (!c || !host) return;
    const w = host.clientWidth > 0 ? host.clientWidth : 1200;
    setWidth(w);
    c.width = Math.floor(w * dpr);
    c.height = Math.floor(height * dpr);
    c.style.width = `${w}px`;
    c.style.height = `${height}px`;
  }, [height]);

  // Observe container for width changes
  useEffect(() => {
    const host = containerRef.current; if (!host) return;
    const RO = (window as any).ResizeObserver;
    if (!RO) return; // skip if not supported (should be in modern browsers)
    const ro = new RO((entries: any[]) => {
      const entry = entries?.[0]; const w = entry?.contentRect?.width || host.clientWidth;
      if (w && Math.abs(w - width) > 2) {
        const dpr = devicePixelRatioRef.current; const c = canvasRef.current; if (c) {
          c.width = Math.floor(w * dpr); c.style.width = `${w}px`; setWidth(w);
        }
      }
    });
    ro.observe(host);
    return () => { ro.disconnect(); };
  }, [width]);

  // Seed with recent blocks
  useEffect(() => {
    (async () => {
      try {
        const res = await fetch(`${apiUrl}/api/blocks?limit=${maxCols}&offset=0`);
        const json = await res.json();
        const blocks = Array.isArray(json?.blocks) ? json.blocks : [];
        const seeded: BlockCol[] = [];
        for (const b of blocks) {
          // Attempt to derive program mix from likely fields
          const mix: ProgramMix = {};
          const cand = b.program_mix || b.program_counts || b.program_id_jsonb || b.programs || b.programs_top || null;
          if (cand && typeof cand === 'object') {
            const entries = Array.isArray(cand) ? cand : Object.entries(cand);
            for (const [pid, val] of entries as any[]) {
              const n = typeof val === 'number' ? val : (val?.count ?? val?.share ?? 0);
              if (n > 0) mix[String(pid)] = n;
            }
          }
          seeded.push({ height: b.height, ts: Date.parse(b.timestamp || b.ts || new Date().toISOString()), mix, txs: b.transaction_count });
        }
        updateModel(seeded);
      } catch {}
    })();
  }, [apiUrl, maxCols]);

  // WS for live blocks
  useEffect(() => {
    let ws: WebSocket | null = null;
    let lastMsgAt = Date.now();
    let hbTimer: any;
    try {
      const envUrl = process.env.NEXT_PUBLIC_WS_URL as string | undefined;
      const wsUrl = (() => {
        if (envUrl && envUrl.length > 0) {
          try { const u = new URL(envUrl); if (!u.pathname || u.pathname === '/') u.pathname = '/ws'; return u.toString(); } catch { return ''; }
        }
        try { const u = new URL(apiUrl); u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:'; u.pathname = '/ws'; return u.toString(); } catch { return ''; }
      })();
      if (!wsUrl) return;
      ws = new WebSocket(wsUrl);
      ws.onopen = () => {
        console.log('[Heatstrip][WS] open', wsUrl);
        ws?.send(JSON.stringify({ method: 'subscribe', params: { topic: 'block_activity', filter: {} }, request_id: 'ui_heatstrip_activity' }));
        ws?.send(JSON.stringify({ method: 'subscribe', params: { topic: 'block', filter: {} }, request_id: 'ui_program_heatstrip' }));
        // heartbeat to detect silence
        hbTimer = setInterval(() => {
          const since = Date.now() - lastMsgAt;
          if (since > 15000) {
            console.warn('[Heatstrip][WS] no block messages for', Math.round(since / 1000), 's');
          }
        }, 5000);
      };
      ws.onmessage = (ev) => {
        try {
          const m = JSON.parse(ev.data);
          const topic = m.topic || m?.result?.topic;
          const d = m.data || m?.result?.data;
          if (topic === 'block_activity') {
            const height = d?.height ?? d?.block_height ?? 0;
            const txs = d?.transaction_count ?? 0;
            const mix = d?.program_counts || {};
            updateModel([{ height, ts: Date.now(), mix, txs }]);
            if (txs > 0) console.log('[Heatstrip][WS][activity]', height, txs, Object.keys(mix).length);
            return;
          }
          if (topic !== 'block') return;
          const height = d?.height ?? d?.block_height ?? 0;
          const txs = d?.transaction_count ?? 0;
          const ts = Date.now();
          lastMsgAt = ts;
          // Derive program mix from various possible fields
          const source = pick(d, ['program_mix', 'program_counts', 'program_id_jsonb', 'programs', 'programs_top', 'instructions_by_program']);
          const mix: ProgramMix = {};
          if (source && typeof source === 'object') {
            const entries = Array.isArray(source) ? source : Object.entries(source);
            for (const [pid, val] of entries as any[]) {
              const n = typeof val === 'number' ? val : (val?.count ?? val?.share ?? 0);
              if (n > 0) mix[String(pid)] = n;
            }
          }
          if (txs > 0) {
            console.log('[Heatstrip][WS] block', height, 'txs', txs, 'programs', Object.keys(mix).length);
          } else {
            console.debug('[Heatstrip][WS] block', height, 'no txs');
          }
          updateModel([{ height, ts, mix, txs }]);
        } catch {}
      };
      ws.onclose = () => {
        // soft-reconnect every 5s
        setTimeout(() => {
          // trigger effect restart by toggling a state
          setCols((prev) => [...prev]);
        }, 5000);
        if (hbTimer) clearInterval(hbTimer);
      };
    } catch {}
    return () => { try { ws?.close(); } catch {}; if (hbTimer) clearInterval(hbTimer); };
  }, [apiUrl]);

  const updateModel = (incoming: BlockCol[]) => {
    setCols((prev) => {
      const merged = [...prev, ...incoming];
      // Trim to window
      const trimmed = merged.slice(-maxCols);
      // Recompute global counts for top programs
      const g = globalCountsRef.current; g.clear();
      for (const c of trimmed) {
        for (const [pid, n] of Object.entries(c.mix)) g.set(pid, (g.get(pid) || 0) + Number(n));
      }
      const ranked = Array.from(g.entries()).sort((a, b) => b[1] - a[1]).slice(0, maxPrograms).map(([pid]) => pid);
      if (ranked.length === 0) {
        setUseTxFallback(true);
        setTopPrograms(['__TX__']);
      } else {
        setUseTxFallback(false);
        setTopPrograms(ranked);
      }
      return trimmed;
    });
  };

  // Draw
  useEffect(() => {
    const c = canvasRef.current; if (!c) return;
    const ctx = c.getContext('2d'); if (!ctx) return;
    const dpr = devicePixelRatioRef.current;
    ctx.resetTransform?.();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const padding = { l: 10, r: 10, t: 8, b: 8 };
    const plotW = Math.max(10, width - padding.l - padding.r);
    const plotH = Math.max(10, height - padding.t - padding.b);
    // Use floating column width to ensure exact right-edge coverage without gap
    const colWF = plotW / Math.max(1, maxCols);
    const startX = padding.l;

    // Labels area removed for cleaner layout; optional legend can be added later
    const rowH = plotH / Math.max(1, topPrograms.length || 1);

    // Draw cells from left→right (oldest to newest)
    // For TX fallback, precompute max for normalization
    let maxTx = 1;
    if (useTxFallback) {
      for (const ccol of cols) { maxTx = Math.max(maxTx, Number(ccol.txs || 0)); }
    }

    // Draw exactly maxCols columns, padding with empty grid for older slots
    for (let ci = 0; ci < maxCols; ci++) {
      const x = startX + Math.floor(ci * colWF);
      const nextX = startX + Math.floor((ci + 1) * colWF);
      const colWpx = Math.max(1, nextX - x);
      const si = cols.length - maxCols + ci;
      const col = si >= 0 ? cols[si] : undefined;
      // Compute sum to transform raw counts to share
      let sum = 0; if (col) { for (const v of Object.values(col.mix)) sum += Number(v); }
      for (let ri = 0; ri < topPrograms.length; ri++) {
        const pid = topPrograms[ri];
        const y = padding.t + ri * rowH;
        let alpha = 0.15;
        if (useTxFallback && pid === '__TX__') {
          const t = Number(col?.txs || 0);
          const shareTx = maxTx > 0 ? t / maxTx : 0;
          ctx.fillStyle = '#19e3ff';
          alpha = Math.max(0.12, Math.min(1, shareTx * 1.2));
        } else {
          const val = Number(col?.mix?.[pid] || 0);
          const share = sum > 0 ? val / sum : 0;
          if (share <= 0) {
            ctx.fillStyle = 'rgba(255,255,255,0.04)';
          } else {
            ctx.fillStyle = colorForProgram(pid);
          }
          alpha = Math.max(0.12, Math.min(1, share * 1.2));
        }
        ctx.globalAlpha = alpha;
        ctx.fillRect(x, y, colWpx, Math.max(1, Math.floor(rowH) - 1));
        ctx.globalAlpha = 1;
      }
    }

    // No text overlays; purely visual heatstrip
  }, [cols, topPrograms, width, height, maxCols]);

  return (
    <div ref={containerRef} style={{ width: '100%', height, border: '1px solid rgba(255,255,255,0.06)', background: '#0a0c10', overflow: 'hidden', margin: '0 0 18px 0' }}>
      <canvas ref={canvasRef} />
    </div>
  );
}
