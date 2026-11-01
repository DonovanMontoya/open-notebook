// GPT Live voice conversations, with the same session settings as the
// server's /live/session (api/routers/live.py): voice only, no notebook
// content, reasoning delegated to an OpenAI chat model.
import type { LiveVoice } from './backend.ts'

export function livePayload(offerSdp: string, voice: LiveVoice, backendModel: string) {
  return {
    session: {
      model: 'gpt-live-1',
      audio: { output: { voice } },
      instructions:
        'You are an AI voice study companion. Keep replies concise, explain one idea at a time, and allow interruptions. You have no access to notebook sources. Delegate reasoning questions to the backend. Never claim to have read notebook documents.',
      delegation: {
        type: 'responses',
        responses: {
          model: backendModel,
          max_output_tokens: 4096,
          instructions:
            'Help with the spoken conversation. You have no notebook documents or source access. Be clear about this limitation when asked about a notebook.',
        },
      },
    },
    transport: { type: 'webrtc', sdp: offerSdp },
  }
}

/** The server only delegates to these OpenAI families; the app does the same. */
export const isLiveBackend = (model: string) => /^gpt-(6|5|4)/.test(model)
