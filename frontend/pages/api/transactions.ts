import type { NextApiRequest, NextApiResponse } from 'next'

type Transaction = {
  signature: string
  block_height: number
  timestamp: string
  fee: number
  status: string
}

type TransactionsResponse = {
  transactions: Transaction[]
  total: number
  page: number
  limit: number
}

export default function handler(
  req: NextApiRequest,
  res: NextApiResponse<TransactionsResponse>
) {
  const { page = 0, limit = 20 } = req.query
  
  // For now, return mock data
  // In production, this would query the database
  const mockTransactions: Transaction[] = Array.from({ length: Number(limit) }, (_, i) => ({
    signature: `mock_signature_${i}_${Math.random().toString(36).substring(7)}`,
    block_height: 12345 - (Number(page) * Number(limit)) - i,
    timestamp: new Date(Date.now() - i * 60000).toISOString(),
    fee: Math.floor(Math.random() * 1000) / 1000, // 0.001 to 1.000 ARCH
    status: ['confirmed', 'pending', 'failed'][Math.floor(Math.random() * 3)]
  }))

  res.status(200).json({
    transactions: mockTransactions,
    total: 67890,
    page: Number(page),
    limit: Number(limit)
  })
}
