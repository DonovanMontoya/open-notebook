// GPT Live voice conversations. The session shape matches the server's /live/session
// (api/routers/live.py). The server's sessions are voice only; on device the app also
// gives the session the notebook's material, so the conversation can be about it.
import type { LiveVoice } from './backend.ts'

/** What a live session knows about the notebook. */
export interface LiveMaterial {
  notebook: string
  /** Short: source titles and summaries, for the voice model itself. */
  overview: string
  /** The full chat context (sources, excerpts and notes), for the model it delegates reasoning to. */
  full: string
}

const VOICE_ONLY =
  'You are an AI voice study companion. Keep replies concise, explain one idea at a time, and allow interruptions. You have no access to notebook sources. Delegate reasoning questions to the backend. Never claim to have read notebook documents.'
const VOICE_ONLY_BACKEND =
  'Help with the spoken conversation. You have no notebook documents or source access. Be clear about this limitation when asked about a notebook.'

const SPOKEN =
  'This is a spoken conversation, so answer in plain speech: no Markdown, lists or brackets. When you use a source, name it in words, like "the FTC report says".'

export function livePayload(offerSdp: string, voice: LiveVoice, backendModel: string, material?: LiveMaterial) {
  return {
    session: {
      model: 'gpt-live-1',
      audio: { output: { voice } },
      instructions: material
        ? [
            `You are a voice study companion for the notebook "${material.notebook}". Keep replies concise, explain one idea at a time, and allow interruptions.`,
            'Below is an overview of the notebook: its sources and what each covers. For anything that needs the details of a source, a quote, a comparison or careful reasoning, delegate to the backend, which has the full text of the notebook.',
            SPOKEN,
            material.overview,
          ].join('\n\n')
        : VOICE_ONLY,
      delegation: {
        type: 'responses',
        responses: {
          model: backendModel,
          max_output_tokens: 4096,
          instructions: material ? `${material.full}\n\n${SPOKEN} Keep answers short enough to say aloud, a few sentences unless asked for more.` : VOICE_ONLY_BACKEND,
        },
      },
    },
    transport: { type: 'webrtc', sdp: offerSdp },
  }
}

/** The server only delegates to these OpenAI families; the app does the same. */
export const isLiveBackend = (model: string) => /^gpt-(6|5|4)/.test(model)
