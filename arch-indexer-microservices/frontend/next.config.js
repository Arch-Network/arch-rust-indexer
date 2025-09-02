/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: "standalone",
  async rewrites() {
    const base = process.env.NEXT_PUBLIC_API_URL;
    // Emit startup diagnostics so we can confirm what the server thinks the API base is
    // These appear in CloudWatch logs for the frontend task.
    console.log(
      `[next] NEXT_PUBLIC_API_URL=${process.env.NEXT_PUBLIC_API_URL || '(unset)'} NEXT_PUBLIC_WS_URL=${process.env.NEXT_PUBLIC_WS_URL || '(unset)'}`
    );
    console.log(`[next] Rewrites base=${base || '(empty)'} -> proxy /api/* and /ws when set`);
    // If a public API URL is provided, proxy /api/* and /ws to it.
    // Otherwise, use same-origin (no rewrites) and rely on ALB path routing.
    if (!base) return [];
    return [
      { source: "/api/:path*", destination: `${base}/api/:path*` },
      { source: "/ws", destination: `${base}/ws` },
    ];
  },
};

module.exports = nextConfig;
