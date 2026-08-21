"""Small OpenAI-compatible client boundary used by the BFF worker."""

from __future__ import annotations


class OpenAICompatibleClient:
    def __init__(self, api_key: str, base_url: str, model: str):
        try:
            from openai import OpenAI
        except ImportError as exc:  # pragma: no cover - deployment dependency.
            raise RuntimeError("openai package is required for configured LLM calls") from exc
        self._client = OpenAI(api_key=api_key, base_url=base_url)
        self._model = model

    def generate(self, system_prompt: str, user_payload: str, *, timeout: float, max_tokens: int) -> str:
        response = self._client.chat.completions.create(
            model=self._model,
            messages=[
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_payload},
            ],
            temperature=0,
            max_tokens=max_tokens,
            response_format={"type": "json_object"},
            timeout=timeout,
        )
        content = response.choices[0].message.content
        if not content:
            raise ValueError("LLM returned empty content")
        return content
