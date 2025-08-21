/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: "standalone",
  async rewrites() {
    return [
      {
        source: "/api/:path*",
        destination: "http://api-server:3001/api/:path*",
      },
      {
        source: "/ws",
        destination: "http://api-server:3001/ws",
      },
    ];
  },
};

module.exports = nextConfig;
