import React, { useEffect, useState } from 'react';
import Layout from '../components/Layout';
import styles from '../styles/Home.module.css';

export default function SettingsPage() {
  const [timezone, setTimezone] = useState<string>('local');
  const [meme, setMeme] = useState<boolean>(false);

  useEffect(() => {
    try {
      const saved = typeof window !== 'undefined' ? window.localStorage.getItem('tz') : null;
      if (saved) setTimezone(saved);
      else setTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC');
    } catch {}
    try {
      const mm = typeof window !== 'undefined' ? window.localStorage.getItem('memeMode') : null;
      setMeme(mm === '1' || mm === 'true');
    } catch {}
  }, []);

  const save = () => {
    try { window.localStorage.setItem('tz', timezone); } catch {}
    alert('Saved');
  };

  const toggleMeme = () => {
    const next = !meme;
    setMeme(next);
    try {
      if (typeof document !== 'undefined') {
        const html = document.documentElement;
        if (next) html.classList.add('meme-mode'); else html.classList.remove('meme-mode');
      }
      if (typeof window !== 'undefined') {
        window.localStorage.setItem('memeMode', next ? '1' : '0');
      }
    } catch {}
  };

  return (
    <Layout>
      <section className={styles.searchSection}>
        <h2>Settings</h2>
        <div className={styles.blockDetails}>
          <div className={styles.detailRow}>
            <strong>Time Zone</strong>
            <select
              value={timezone}
              onChange={(e) => setTimezone(e.target.value)}
              style={{ background: 'var(--panel)', color: 'var(--text)', border: '1px solid rgba(255,255,255,0.12)', padding: '8px', fontSize: 12 }}
            >
              <option value={Intl.DateTimeFormat().resolvedOptions().timeZone}>{Intl.DateTimeFormat().resolvedOptions().timeZone}</option>
              <option value="UTC">UTC</option>
              <option value="America/New_York">America/New_York (ET)</option>
              <option value="America/Chicago">America/Chicago (CT)</option>
              <option value="America/Denver">America/Denver (MT)</option>
              <option value="America/Los_Angeles">America/Los_Angeles (PT)</option>
              <option value="Europe/London">Europe/London</option>
              <option value="Asia/Tokyo">Asia/Tokyo</option>
            </select>
            <button className={styles.refreshButton} onClick={save}>Save</button>
          </div>
          <div className={styles.detailRow}>
            <strong>Meme Mode</strong>
            <button
              role="switch"
              aria-checked={meme}
              className={`${styles.refreshButton} ${styles.pillToggle}`}
              onClick={toggleMeme}
            >
              <span role="img" aria-label="meme">😎</span>
              <span>{meme ? 'On' : 'Off'}</span>
            </button>
          </div>
          <div className={styles.detailRow}>
            <strong>Note</strong>
            <span className={styles.muted}>Timestamps are sourced from the chain as UTC and rendered in your selected time zone. If times appear in the future, your system clock or selected time zone may be misconfigured.</span>
          </div>
        </div>
      </section>
    </Layout>
  );
}
