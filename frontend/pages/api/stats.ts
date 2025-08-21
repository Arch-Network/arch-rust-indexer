import type { NextApiRequest, NextApiResponse } from 'next'

type Stats = {
  total_blocks: number
  total_transactions: number
  latest_block: number
  total_size: number
}

export default function handler(
  req: NextApiRequest,
  res: NextApiResponse<Stats>
) {
  // For now, return mock data
  // In production, this would query the database
  res.status(200).json({
    total_blocks: 12345,
    total_transactions: 67890,
    latest_block: 12345,
    total_size: 1024 * 1024 * 100 // 100 MB
  })
}
