import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import apiClient from '@/lib/api/client'
import { QUERY_KEYS } from '@/lib/api/query-client'
import { toast } from 'sonner'
import { useTranslation } from '@/lib/hooks/use-translation'

export interface StudyItem {
  question: string
  answer: string
  options: string[]
  correct_index: number | null
  source_ids: string[]
}
export interface StudyDeck {
  version: 1
  kind: 'flashcards' | 'quiz'
  title: string
  items: StudyItem[]
}
export interface StudyRequest {
  kind: StudyDeck['kind']
  source_ids: string[]
  count: number
  difficulty: 'easy' | 'medium' | 'hard'
  language: string
  instructions: string
}
export function useStudio(notebookId: string) {
  const client = useQueryClient()
  const { t } = useTranslation()
  const url = `/notebooks/${encodeURIComponent(notebookId)}/studio`
  const decks = useQuery({
    queryKey: QUERY_KEYS.studio(notebookId),
    queryFn: async () => (await apiClient.get<StudyDeck[]>(url)).data,
    enabled: !!notebookId,
  })
  const generate = useMutation({
    mutationFn: async (request: StudyRequest) =>
      (await apiClient.post<StudyDeck>(`${url}/generate`, request)).data,
    retry: false,
    onSuccess: () => {
      client.invalidateQueries({ queryKey: QUERY_KEYS.studio(notebookId) })
      client.invalidateQueries({ queryKey: QUERY_KEYS.notes(notebookId) })
      toast.success(t('studio.saved'))
    },
    onError: () => toast.error(t('studio.failed')),
  })
  return { decks, generate }
}
