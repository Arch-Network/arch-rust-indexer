import React, { ReactNode } from 'react';
import Link from 'next/link';
import styles from '../styles/Home.module.css';
import HeaderSearch from './HeaderSearch';

type LayoutProps = {
  rightActions?: ReactNode;
  children: ReactNode;
};

export default function Layout({ rightActions, children }: LayoutProps) {
  return (
    <div className={styles.container}>
      <div className="scanline" />
      <header className={styles.header}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
          <h1 style={{ marginRight: 8 }}>Arch Explorer</h1>
          <nav className="siteNav">
            <Link href="/">Home</Link>
            <Link href="/blocks">Blocks</Link>
            <Link href="/tx">Transactions</Link>
            <Link href="/programs">Programs</Link>
            <Link href="/tokens">Tokens</Link>
          </nav>
        </div>
        <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
          <HeaderSearch />
          {rightActions}
        </div>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
