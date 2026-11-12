import { useQuery } from '@tanstack/react-query'
import apiClient from '@/lib/api/client'
export interface UsageGroup { model: string; feature: string; calls: number; input_tokens: number; output_tokens: number; audio_seconds: number; estimated_usd: number; unpriced: number }
export interface UsageEvent { id: string; model: string; feature: string; created: string; estimated_usd: number | null; basis: string }
export function useUsage(days: number) {
  return useQuery({ queryKey: ['usage', days], queryFn: async () => (await apiClient.get<{ estimated_usd: number; calls: number; unpriced: number; groups: UsageGroup[]; recent: UsageEvent[]; pricing_date: string }>('/usage', { params: { days } })).data, refetchInterval: 30000, retry: false })
}
