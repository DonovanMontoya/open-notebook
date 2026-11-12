// Default podcast profiles, copied from the Open Notebook server so episodes are styled the same in both modes.

import type { EpisodeProfileSeed, SpeakerProfileSeed } from './profiles.ts'

export const DEFAULT_SPEAKER_PROFILES: SpeakerProfileSeed[] = [
  {
    name: "business_panel",
    description: "Business analysis panel with diverse perspectives",
    voiceKind: 'openai',
    voiceModel: 'gpt-4o-mini-tts',
    speakers: [
      { name: "Marcus Thompson", voice: "echo", backstory: "Former McKinsey consultant, now startup advisor. Expert in strategic analysis and market dynamics.", personality: "Strategic thinker, data-driven, excellent at identifying key insights and implications" },
      { name: "Elena Vasquez", voice: "shimmer", backstory: "Serial entrepreneur and investor. Focuses on practical implementation and execution.", personality: "Action-oriented, pragmatic, brings startup experience and execution focus" },
      { name: "Johny Bing", voice: "ash", backstory: "Youtube celebrity and business mogul. Focuses on practical implementation and execution.", personality: "Controversial, likes to question ideas and concepts. He brings a fresh perspective and always has a point to make." },
    ],
  },
  {
    name: "solo_expert",
    description: "Single expert for educational content",
    voiceKind: 'openai',
    voiceModel: 'gpt-4o-mini-tts',
    speakers: [
      { name: "Professor Sarah Kim", voice: "marin", backstory: "Distinguished professor and researcher. Has a gift for making complex topics accessible to broad audiences.", personality: "Patient teacher, uses analogies and examples, breaks down complex concepts step by step" },
    ],
  },
  {
    name: "tech_experts",
    description: "Two technical experts for tech discussions",
    voiceKind: 'openai',
    voiceModel: 'gpt-4o-mini-tts',
    speakers: [
      { name: "Dr. Alex Chen", voice: "marin", backstory: "Senior AI researcher and former tech lead at major companies. Specializes in making complex technical concepts accessible.", personality: "Analytical, clear communicator, asks probing questions to dig deeper into technical details" },
      { name: "Jamie Rodriguez", voice: "alloy", backstory: "Full-stack engineer and tech entrepreneur. Loves practical applications and real-world implementations.", personality: "Enthusiastic, practical-minded, great at explaining implementation details and trade-offs" },
    ],
  },
]

export const DEFAULT_EPISODE_PROFILES: EpisodeProfileSeed[] = [
  {
    name: "business_analysis",
    description: "Business-focused analysis and discussion",
    speakerProfileName: "business_panel",
    briefing: "Analyze the provided content from a business perspective. Discuss market implications, strategic insights, competitive advantages, and actionable business intelligence.",
    segments: 6,
  },
  {
    name: "solo_expert",
    description: "Single expert explaining complex topics",
    speakerProfileName: "solo_expert",
    briefing: "Create an educational explanation of the provided content. Break down complex concepts into digestible segments, use analogies and examples, and maintain an engaging teaching style.",
    segments: 4,
  },
  {
    name: "tech_discussion",
    description: "Technical discussion between 2 experts",
    speakerProfileName: "tech_experts",
    briefing: "Create an engaging technical discussion about the provided content. Focus on practical insights, real-world applications, and detailed explanations that would interest developers and technical professionals.",
    segments: 5,
  },
]
