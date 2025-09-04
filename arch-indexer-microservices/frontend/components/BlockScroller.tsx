import React, { useEffect, useRef, useState } from 'react';

type BlockCard = {
  id: string;
  height?: number;
  txs?: number;
  timestamp?: string;
  miner?: string;
};

type Props = {
  apiUrl: string;
};

export default function BlockScroller({ apiUrl }: Props) {
  const [items, setItems] = useState<BlockCard[]>([]);
  const seenRef = useRef<Set<string>>(new Set());
  const queueRef = useRef<BlockCard[]>([]);
  const rowRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<number | null>(null);
  const pausedRef = useRef<boolean>(false);
  const offsetRef = useRef<number>(0);
  const msgCountRef = useRef<number>(0);

  useEffect(() => {
    let ws: WebSocket | null = null;
    try {
      const envUrl = process.env.NEXT_PUBLIC_WS_URL as string | undefined;
      const normalizeWsUrl = (raw: string): string => {
        try {
          // Ensure protocol and path are correct even if env omits /ws
          const u = new URL(raw);
          if (u.protocol !== 'ws:' && u.protocol !== 'wss:') {
            u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:';
          }
          if (!u.pathname || u.pathname === '/' ) {
            u.pathname = '/ws';
          }
          u.search = '';
          u.hash = '';
          return u.toString();
        } catch {
          return '';
        }
      };
      const deriveWsFromApi = (api: string): string => {
        try {
          const u = new URL(api);
          u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:';
          // Always mount WS at /ws regardless of API path (e.g. /api)
          u.pathname = '/ws';
          u.search = '';
          u.hash = '';
          return u.toString();
        } catch {
          return '';
        }
      };
      const wsUrl = envUrl && envUrl.length > 0 ? normalizeWsUrl(envUrl) : deriveWsFromApi(apiUrl || '');
      if (!wsUrl) return;
      console.info('[BlockScroller] Connecting WebSocket →', wsUrl);
      ws = new WebSocket(wsUrl);
      ws.onopen = () => {
        const sub = JSON.stringify({ method: 'subscribe', params: { topic: 'block', filter: {} }, request_id: 'ui_block_scroller' });
        ws?.send(sub);
        console.info('[BlockScroller] WebSocket open; subscription sent');
      };
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          const topic = msg.topic || msg?.result?.topic;
          const data = msg.data || msg?.result?.data;
          if (topic === 'block' && data) {
            const rawHeight = data.height ?? data.block_height ?? data.number;
            const hash = data.hash ?? data.block_hash ?? data.id;
            const id = String(rawHeight ?? hash ?? `${Date.now()}_${Math.random()}`);
            const height = typeof rawHeight === 'number' ? rawHeight : undefined;
            const txs = data.transaction_count ?? data.txs?.length ?? 0;
            const timestamp = data.timestamp ?? data.time ?? null;
            const card: BlockCard = { id, height, txs, timestamp: timestamp || undefined };
            if (!seenRef.current.has(id)) {
              seenRef.current.add(id);
              queueRef.current.push(card);
              msgCountRef.current += 1;
              if (msgCountRef.current % 5 === 1) {
                console.info('[BlockScroller] Enqueued block', { id, height, txs, queueLen: queueRef.current.length });
              }
              // Kick the animation loop immediately if idle
              if (!pausedRef.current && !animRef.current) {
                console.info('[BlockScroller] Animation idle → starting next batch');
                playNextBatch();
              }
            }
          }
        } catch {}
      };
    } catch {}
    return () => { try { ws?.close(); } catch {} };
  }, [apiUrl]);

  useEffect(() => {
    // Seed with recent blocks on mount
    (async () => {
      try {
        const res = await fetch(`${apiUrl}/api/blocks?limit=10&offset=0`);
        const json = await res.json();
        const blocks = Array.isArray(json?.blocks) ? json.blocks : [];
        const seeded = blocks.map((b: any) => ({ id: String(b.height ?? b.hash ?? b.id), height: b.height, txs: b.transaction_count, timestamp: b.timestamp })) as BlockCard[];
        seeded.forEach(b => seenRef.current.add(b.id));
        setItems(seeded);
        // play initial sweep once by scheduling them in the queue; we won't loop endlessly
        // Do not enqueue initial seeds; we only animate on NEW blocks from WS
      } catch {}
    })();
  }, [apiUrl]);

  const playNextBatch = () => {
    if (!rowRef.current) return;
    // take next item(s) from queue
    const next = queueRef.current.shift();
    if (!next) {
      return; // nothing to animate
    }
    // prepend the card and animate a left shift equal to card width + gap
    let cardWidth = 212; // fallback
    try {
      const row = rowRef.current as HTMLDivElement;
      const first = row.firstElementChild as HTMLElement | null;
      const styles = row ? getComputedStyle(row) : (null as any);
      const gapStr = styles?.gap || styles?.columnGap || '12px';
      const gap = parseInt(gapStr, 10) || 12;
      const firstWidth = first?.offsetWidth || 200;
      cardWidth = firstWidth + gap;
    } catch {}
    setItems((prev) => {
      const updated = [next, ...prev];
      // maintain uniqueness by height
      const uniq: BlockCard[] = [];
      const seen = new Set<string>();
      for (const b of updated) {
        if (!seen.has(b.id)) { seen.add(b.id); uniq.push(b); }
        if (uniq.length >= 20) break;
      }
      return uniq;
    });
    const start = offsetRef.current;
    const end = start - cardWidth;
    const el = rowRef.current;
    const startTime = performance.now();
    const duration = 800; // ms per card
    console.info('[BlockScroller] Playing batch → translate by', cardWidth, 'queue left', queueRef.current.length);
    const tick = (t: number) => {
      const p = Math.min(1, (t - startTime) / duration);
      const val = start + (end - start) * p;
      offsetRef.current = val;
      el.style.transform = `translateX(${val}px)`;
      if (p < 1) {
        animRef.current = requestAnimationFrame(tick);
      } else {
        // reset offset when a full card has moved
        offsetRef.current = 0;
        el.style.transform = 'translateX(0px)';
        // recursively play the next batch if any
        if (queueRef.current.length > 0) {
          playNextBatch();
        } else {
          animRef.current = null;
          console.info('[BlockScroller] Queue drained; animation idle');
        }
      }
    };
    animRef.current = requestAnimationFrame(tick);
  };

  useEffect(() => {
    // batch animation loop: only play when queue has items and not paused
    const loop = () => {
      if (!pausedRef.current && queueRef.current.length > 0 && !animRef.current) {
        playNextBatch();
      }
      requestAnimationFrame(loop);
    };
    const id = requestAnimationFrame(loop);
    return () => { cancelAnimationFrame(id); if (animRef.current) cancelAnimationFrame(animRef.current); };
  }, []);

  // Handle incoming WS messages → only enqueue unseen heights
  useEffect(() => {
    const id = setInterval(() => {
      // Drain queue if animation somehow got stuck
      if (!animRef.current && queueRef.current.length > 0) {
        playNextBatch();
      }
    }, 2000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="hScroller" onMouseEnter={() => (pausedRef.current = true)} onMouseLeave={() => (pausedRef.current = false)}>
      <div ref={rowRef} className="hScrollerRow">
        {items.map((b) => (
          <a key={b.height} className="hBlockCard" href={`/blocks/${b.height}`}>
            <h4>Block {b.height}</h4>
            <div className="hBlockMeta">
              <span>{b.txs ?? 0} tx</span>
              <span>{b.timestamp ? new Date(b.timestamp).toLocaleTimeString() : '—'}</span>
            </div>
          </a>
        ))}
      </div>
    </div>
  );
}
