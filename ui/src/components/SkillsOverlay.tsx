// components/SkillsOverlay.tsx — skills list panel
// Source: Claude Code doc 29 §6.3 — SkillsMenu
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Overlay } from './Overlay.js';
import { getSkills } from '../services/api.js';

export function SkillsOverlay() {
  const [skills, setSkills] = useState<Array<{ name: string; description?: string }>>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getSkills().then(s => { setSkills(s); setLoading(false); }).catch(() => setLoading(false));
  }, []);

  return (
    <Overlay title="Skills" hint="Esc to close">
      {loading ? (
        <Text color={theme.inactive}>Loading…</Text>
      ) : skills.length === 0 ? (
        <Text color={theme.inactive}>(No skills available)</Text>
      ) : (
        <Box flexDirection="column">
          {skills.map(s => (
            <Box key={s.name} flexDirection="row">
              <Box width={20}><Text color={theme.claude} bold>{s.name}</Text></Box>
              <Text color={theme.text}>{s.description ?? ''}</Text>
            </Box>
          ))}
        </Box>
      )}
    </Overlay>
  );
}
