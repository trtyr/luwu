// Custom text input — no external dependency, guaranteed Bun-compatible
import React from 'react';
import { Text, useInput } from 'ink';

interface TextInputProps {
  value: string;
  onChange: (v: string) => void;
  onSubmit: (v: string) => void;
  placeholder?: string;
}

export function CustomInput({ value, onChange, onSubmit, placeholder }: TextInputProps) {
  useInput((input, key) => {
    if (key.return) {
      onSubmit(value);
      return;
    }
    if (key.backspace || key.delete) {
      onChange(value.slice(0, -1));
      return;
    }
    if (key.ctrl && input === 'u') {
      onChange('');
      return;
    }
    // Regular character — ignore control sequences
    if (input && !key.ctrl && !key.meta && input.length === 1 && input >= ' ') {
      onChange(value + input);
    }
  });

  const display = value || (placeholder ? '' : '');
  return (
    <Text>
      <Text color="gray">{!value && placeholder ? placeholder : ''}</Text>
      <Text>{display}</Text>
      <Text color="cyan">{'\u2588'}</Text>
    </Text>
  );
}
