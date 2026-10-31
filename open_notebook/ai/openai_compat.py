"""Compatibility for GPT-6 until Esperanto recognizes the model family."""

from esperanto.providers.llm.openai import OpenAILanguageModel


class GPT6LanguageModel(OpenAILanguageModel):
    def _is_reasoning_model(self) -> bool:
        return True

    def to_langchain(self):
        model = super().to_langchain()
        # GPT-6 tool calls require Responses. Sampling controls are unsupported.
        model.use_responses_api = True
        model.temperature = None
        model.top_p = None
        return model
