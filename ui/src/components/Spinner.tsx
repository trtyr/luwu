// components/Spinner.tsx — Claude Code-style spinner (1:1 verb list from spinnerVerbs.ts)
import React, { useState, useEffect, useRef } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { MessageResponse } from './MessageResponse.js';

// Exact verb list from Claude Code constants/spinnerVerbs.ts
const SPINNER_VERBS = [
  'Accomplishing','Actioning','Actualizing','Architecting','Baking','Beaming',
  "Beboppin'",'Befuddling','Billowing','Blanching','Bloviating','Boogieing',
  'Boondoggling','Booping','Bootstrapping','Brewing','Bunning','Burrowing',
  'Calculating','Canoodling','Caramelizing','Cascading','Catapulting',
  'Cerebrating','Channeling','Channelling','Choreographing','Churning',
  'Clauding','Coalescing','Cogitating','Combobulating','Composing','Computing',
  'Concocting','Considering','Contemplating','Cooking','Crafting','Creating',
  'Crunching','Crystallizing','Cultivating','Deciphering','Deliberating',
  'Determining','Dilly-dallying','Discombobulating','Doing','Doodling',
  'Drizzling','Ebbing','Effecting','Elucidating','Embellishing','Enchanting',
  'Envisioning','Evaporating','Fermenting','Fiddle-faddling','Finagling',
  'Flambéing','Flibbertigibbeting','Flowing','Flummoxing','Fluttering',
  'Forging','Forming','Frolicking','Frosting','Gallivanting','Galloping',
  'Garnishing','Generating','Gesticulating','Germinating','Gitifying',
  'Grooving','Gusting','Harmonizing','Hashing','Hatching','Herding','Honking',
  'Hullaballooing','Hyperspacing','Ideating','Imagining','Improvising',
  'Incubating','Inferring','Infusing','Ionizing','Jitterbugging','Julienning',
  'Kneading','Leavening','Levitating','Lollygagging','Manifesting',
  'Marinating','Meandering','Metamorphosing','Misting','Moonwalking',
  'Moseying','Mulling','Mustering','Musing','Nebulizing','Nesting',
  'Newspapering','Noodling','Nucleating','Orbiting','Orchestrating',
  'Osmosing','Perambulating','Percolating','Perusing','Philosophising',
  'Photosynthesizing','Pollinating','Pondering','Pontificating','Pouncing',
  'Precipitating','Prestidigitating','Processing','Proofing','Propagating',
  'Puttering','Puzzling','Quantumizing','Razzle-dazzling','Razzmatazzing',
  'Recombobulating','Reticulating','Roosting','Ruminating','Sautéing',
  'Scampering','Schlepping','Scurrying','Seasoning','Shenaniganing',
  'Shimmying','Simmering','Skedaddling','Sketching','Slithering','Smooshing',
  'Sock-hopping','Spelunking','Spinning','Sprouting','Stewing','Sublimating',
  'Swirling','Swooping','Symbioting','Synthesizing','Tempering','Thinking',
  'Thundering','Tinkering','Tomfoolering','Topsy-turvying','Transfiguring',
  'Transmuting','Twisting','Undulating','Unfurling','Unravelling','Vibing',
  'Waddling','Wandering','Warping','Whatchamacalliting','Whirlpooling',
  'Whirring','Whisking','Wibbling','Working','Wrangling','Zesting','Zigzagging',
];

const TIPS = [
  'Use /clear to start fresh when switching topics and free up context',
  '↑↓ browse history · / for commands · ctrl+c to cancel',
  'Type /help to see all available commands',
];

function pick<T>(arr: T[]): T { return arr[Math.floor(Math.random() * arr.length)]; }

interface Props {
  phase: string;
  verb?: string;
}

export function Spinner({ phase, verb }: Props) {
  const [mountVerb] = useState(() => pick(SPINNER_VERBS));
  const [tip] = useState(() => pick(TIPS));
  const [thinkingStatus, setThinkingStatus] = useState<'thinking' | number | null>(null);
  const thinkingStartRef = useRef<number | null>(null);

  useEffect(() => {
    let durTimer: ReturnType<typeof setTimeout> | null = null;
    let clearTimer: ReturnType<typeof setTimeout> | null = null;
    if (phase === 'thinking') {
      if (thinkingStartRef.current === null) {
        thinkingStartRef.current = Date.now();
        setThinkingStatus('thinking');
      }
    } else if (thinkingStartRef.current !== null) {
      const duration = Date.now() - thinkingStartRef.current;
      const remaining = Math.max(0, 2000 - duration);
      thinkingStartRef.current = null;
      const showDuration = () => {
        setThinkingStatus(duration);
        clearTimer = setTimeout(() => setThinkingStatus(null), 2000);
      };
      if (remaining > 0) durTimer = setTimeout(showDuration, remaining);
      else showDuration();
    }
    return () => {
      if (durTimer) clearTimeout(durTimer);
      if (clearTimer) clearTimeout(clearTimer);
    };
  }, [phase]);

  if (phase !== 'thinking' && thinkingStatus === null) return null;

  // Random completion verb (from Claude Code turnCompletionVerbs.ts)
  const completionVerb = pick(['Baked', 'Brewed', 'Churned', 'Cogitated', 'Cooked', 'Crunched', 'Sautéed', 'Worked']);

  if (phase !== 'thinking' && typeof thinkingStatus === 'number') {
    return (
      <Box marginLeft={2}>
        <Text color={theme.claude}>✻ </Text>
        <Text color={theme.inactive}>{completionVerb} for </Text>
        <Text color={theme.text} bold>{(thinkingStatus / 1000).toFixed(1)}s</Text>
      </Box>
    );
  }

  const effectiveVerb = verb || mountVerb;

  return (
    <Box flexDirection="column" width="100%" alignItems="flex-start">
      <Box marginTop={1}>
        <Text>
          <Text color={theme.claude}>✻ </Text>
          <Text color={theme.text}>{effectiveVerb}…</Text>
        </Text>
      </Box>
      <MessageResponse>
        <Text dimColor>{tip}</Text>
      </MessageResponse>
    </Box>
  );
}
