import type { AppProps } from 'next/app'
import { useEffect } from 'react'
import Head from 'next/head'
import '../styles/globals.css'

export default function App({ Component, pageProps }: AppProps) {
  // Apply persisted meme mode class on initial load (and route changes)
  useEffect(() => {
    try {
      const saved = typeof window !== 'undefined' ? window.localStorage.getItem('memeMode') : null;
      const html = typeof document !== 'undefined' ? document.documentElement : null;
      if (!html) return;
      if (saved === '1' || saved === 'true') {
        html.classList.add('meme-mode');
      } else {
        html.classList.remove('meme-mode');
      }
    } catch {
      // ignore storage errors (private mode, etc.)
    }
  }, []);

  return (
    <>
      <Head>
        <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
      </Head>
      <Component {...pageProps} />
    </>
  )
}
