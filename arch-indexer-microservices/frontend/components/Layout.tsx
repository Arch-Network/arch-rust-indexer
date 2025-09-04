import React, { ReactNode, useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import styles from '../styles/Home.module.css';
import HeaderSearch from './HeaderSearch';

type LayoutProps = {
  rightActions?: ReactNode;
  children: ReactNode;
};

export default function Layout({ rightActions, children }: LayoutProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [exploreOpen, setExploreOpen] = useState(false);
  const exploreRef = useRef<HTMLDivElement | null>(null);

  // Close the mobile menu on ESC
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { setMenuOpen(false); setExploreOpen(false); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (exploreRef.current && !exploreRef.current.contains(e.target as Node)) {
        setExploreOpen(false);
      }
    };
    document.addEventListener('click', handler);
    return () => document.removeEventListener('click', handler);
  }, []);

  const NavLinks = () => (
    <>
      <Link href="/">Home</Link>
      <Link href="/blocks">Blocks</Link>
      <Link href="/tx">Transactions</Link>
      <Link href="/programs">Programs</Link>
      <Link href="/tokens">Tokens</Link>
    </>
  );

  return (
    <div className={styles.container}>
      <div className="scanline" />
      <header className={styles.header}>
        <div className={styles.brand}>
          <h1><Link href="/">Arch Explorer</Link></h1>
        </div>
        <div className={styles.searchBar}>
          <HeaderSearch />
        </div>
        <div className={styles.actions}>
          <div ref={exploreRef} className={styles.menuGroup}>
            <button
              className={`${styles.menuLink} ${styles.refreshButton}`}
              aria-haspopup="true"
              aria-expanded={exploreOpen}
              onClick={(e) => { e.stopPropagation(); setExploreOpen(v => !v); }}
            >
              Explore ▾
            </button>
            <div className={`${styles.dropdownMenu} ${exploreOpen ? styles.dropdownOpen : ''}`} role="menu">
              <Link href="/blocks" role="menuitem">Blocks</Link>
              <Link href="/tx" role="menuitem">Transactions</Link>
              <Link href="/programs" role="menuitem">Programs</Link>
              <Link href="/tokens" role="menuitem">Tokens</Link>
              <Link href="/settings" role="menuitem">Settings</Link>
            </div>
          </div>
          {rightActions}
          <button
            className={styles.menuButton}
            aria-label={menuOpen ? 'Close menu' : 'Open menu'}
            aria-expanded={menuOpen}
            aria-controls="primary-navigation"
            onClick={() => setMenuOpen(v => !v)}
          >
            ☰
          </button>
        </div>
      </header>
      {menuOpen && (
        <>
          <div className={styles.navOverlay} onClick={() => setMenuOpen(false)} />
          <aside className={`${styles.navDrawer} ${styles.navDrawerOpen}`} role="dialog" aria-modal="true">
            <div className={styles.navDrawerHeader}>
              <h3 className={styles.drawerTitle}>Menu</h3>
              <button className={styles.menuLink} onClick={() => setMenuOpen(false)} aria-label="Close menu">✕</button>
            </div>
            <div className={styles.navDrawerBody}>
              <div className={styles.navSection}>
                <div className={styles.navSectionTitle}>Explore</div>
                <nav className="siteNav" aria-label="Mobile Primary">
                  <Link href="/blocks" onClick={() => setMenuOpen(false)}>Blocks</Link>
                  <Link href="/tx" onClick={() => setMenuOpen(false)}>Transactions</Link>
                  <Link href="/programs" onClick={() => setMenuOpen(false)}>Programs</Link>
                  <Link href="/tokens" onClick={() => setMenuOpen(false)}>Tokens</Link>
                </nav>
              </div>
              <div className={styles.navSection}>
                <div className={styles.navSectionTitle}>Tools</div>
                <div className={styles.navList}>
                  <Link href="/settings" onClick={() => setMenuOpen(false)}>Settings</Link>
                  {rightActions}
                </div>
              </div>
            </div>
          </aside>
        </>
      )}
      <main className={styles.main}>{children}</main>
    </div>
  );
}
