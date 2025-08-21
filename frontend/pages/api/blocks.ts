import type { NextApiRequest, NextApiResponse } from 'next'

type Block = {
  height: number
  hash: string
  timestamp: string
  transactions: number
  size: number
}

type BlocksResponse = {
  blocks: Block[]
  total: number
  page: number
  limit: number
}

export default function handler(
  req: NextApiRequest,
  res: NextApiResponse<BlocksResponse>
) {
  const { page = 0, limit = 20 } = req.query
  
  // For now, return mock data
  // In production, this would query the database
  const mockBlocks: Block[] = Array.from({ length: Number(limit) }, (_, i) => ({
    height: 12345 - (Number(page) * Number(limit)) - i,
    hash: `mock_hash_${12345 - (Number(page) * Number(limit)) - i}_${Math.random().toString(36).substring(7)}`,
    timestamp: new Date(Date.now() - i * 60000).toISOString(),
    transactions: Math.floor(Math.random() * 100),
    size: Math.floor(Math.random() * 1024 * 1024) + 1024 * 100 // 100KB to 1MB
  }))

  res.status(200).json({
    blocks: mockBlocks,
    total: 12345,
    page: Number(page),
    limit: Number(limit)
  })
}
