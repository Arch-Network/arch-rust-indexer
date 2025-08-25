import type { NextApiRequest, NextApiResponse } from 'next'

type SearchResult = {
  type: string
  value: string
  description: string
}

type SearchResponse = {
  results: SearchResult[]
  query: string
}

export default function handler(
  req: NextApiRequest,
  res: NextApiResponse<SearchResponse>
) {
  const { q } = req.query
  
  if (!q || typeof q !== 'string') {
    return res.status(400).json({ results: [], query: '' })
  }

  const query = q.toLowerCase()
  
  // For now, return mock search results
  // In production, this would search the database
  const mockResults: SearchResult[] = []
  
  // Simulate finding blocks
  if (query.match(/^\d+$/)) {
    mockResults.push({
      type: 'Block',
      value: query,
      description: `Block at height ${query}`
    })
  }
  
  // Simulate finding transactions
  if (query.length > 20) {
    mockResults.push({
      type: 'Transaction',
      value: `${query.slice(0,8)}…${query.slice(-8)}`,
      description: 'Transaction signature'
    })
  }
  
  // Simulate finding addresses
  if (query.length > 30 && query.length < 50) {
    mockResults.push({
      type: 'Address',
      value: `${query.slice(0,8)}…${query.slice(-8)}`,
      description: 'Wallet address'
    })
  }
  
  // If no specific matches, add some generic results
  if (mockResults.length === 0) {
    mockResults.push({
      type: 'Search',
      value: query,
      description: `Search results for "${query}"`
    })
  }

  res.status(200).json({
    results: mockResults,
    query: q
  })
}
