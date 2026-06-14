// ModelPicker.tsx — interactive model selection overlay
// Claude Code /model: local-jsx command → Modal Pane with CustomSelect
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Select, type SelectOption } from './Select.js';
import { getModels } from '../services/api.js';
import type { ModelInfo } from '../core/types.js';

interface ModelPickerProps {
  currentModel: string;
  onSelect: (model: string) => void;
  onCancel: () => void;
}

export function ModelPicker({ currentModel, onSelect, onCancel }: ModelPickerProps) {
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getModels()
      .then(m => {
        // Deduplicate by id — server may return same model from multiple providers
        const seen = new Set<string>();
        const unique = m.filter(model => {
          if (seen.has(model.id)) return false;
          seen.add(model.id);
          return true;
        });
        setModels(unique);
      })
      .catch(() => setError('无法获取模型列表'));
  }, []);

  if (error) {
    return (
      <Box flexDirection="column" padding={1}>
        <Text color={theme.warning}>⚠ {error}</Text>
        <Text color={theme.subtle}>按 Esc 返回</Text>
      </Box>
    );
  }

  if (!models) {
    return (
      <Box padding={1}>
        <Text color={theme.subtle}>加载模型列表…</Text>
      </Box>
    );
  }

  if (models.length === 0) {
    return (
      <Box flexDirection="column" padding={1}>
        <Text color={theme.warning}>⚠ 服务器没有可用模型</Text>
        <Text color={theme.subtle}>按 Esc 返回</Text>
      </Box>
    );
  }

  const options: SelectOption[] = models.map(m => ({
    value: m.id,
    label: m.id,
    description: m.id === currentModel ? '← 当前' : undefined,
  }));

  return (
    <Box flexDirection="column" marginTop={1}>
      <Box marginBottom={1} flexDirection="column">
        <Text color={theme.claude} bold>Select model</Text>
        <Text color={theme.inactive}>↑↓ 选择 · Enter 确认 · Esc 取消</Text>
      </Box>
      <Select
        options={options}
        defaultValue={currentModel}
        onSelect={onSelect}
        onCancel={onCancel}
      />
    </Box>
  );
}
