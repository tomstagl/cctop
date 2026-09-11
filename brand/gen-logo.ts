#!/usr/bin/env tsx
// Generates cctop logo candidates via kie.ai Flux Kontext, reusing studio-master's client.
// Run from studio-master:  yarn dotenv -e .env.local -- tsx ../cctop/brand/gen-logo.ts
import { writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { submitFluxTask, waitForFluxTask, downloadFile } from '/Users/tom/code/studio-master/scripts/kie-ai/kie-client';

const OUT = '/Users/tom/code/cctop/brand/candidates';
mkdirSync(OUT, { recursive: true });

const STYLE =
  'Flat vector logo, clean geometric shapes, 2-3 colours maximum, high contrast, ' +
  'centred on a plain solid dark charcoal background #0E1318, no gradients, no photorealism, ' +
  'no extra text, no watermark, crisp edges, suitable as an app icon.';

const variants: Record<string, string> = {
  spark_beard:
    'A logo mascot: the Claude Code asterisk spark symbol (an eight-pointed rounded asterisk, ' +
    'warm terracotta orange #D97757) wearing the iconic ZZ Top look — a very long braided ' +
    'chest-length beard and small round dark sunglasses. The beard hangs down below the asterisk. ' +
    'Minimal, witty, single icon. Teal accent #4CC2C2 on the sunglasses reflection. ' + STYLE,
  terminal_beard:
    'A logo: a rounded terminal window icon (dark box with a thin light border and a small ' +
    'blinking cursor bar and a horizontal bar chart of three teal #4CC2C2 bars inside, like an ' +
    'htop CPU meter). The terminal window has a long ZZ Top style beard hanging from its bottom ' +
    'edge and round dark sunglasses across its top. The beard is drawn in terracotta orange #D97757. ' + STYLE,
  wordmark:
    'A wordmark logo reading exactly "cctop" in lowercase, bold condensed sans-serif letters in ' +
    'off-white, where the two "c" letters wear tiny round dark sunglasses and share one long ' +
    'ZZ Top style beard in terracotta orange #D97757 hanging below the baseline. A small teal ' +
    '#4CC2C2 asterisk spark sits above the "o" like an accent. ' + STYLE,
  cowboy_bars:
    'A logo icon: a stack of five horizontal meter bars of decreasing length (like a btop gauge) ' +
    'in teal #4CC2C2, arranged so their silhouette forms a long ZZ Top style beard hanging below ' +
    'a pair of round dark sunglasses drawn in terracotta orange #D97757. Reads both as a bar ' +
    'chart and as a bearded face. ' + STYLE,
  gauge_beard_v2:
    'A logo icon: a stack of five horizontal meter bars of decreasing length (like a btop gauge), ' +
    'all in terracotta orange #D97757, arranged so their silhouette forms a long ZZ Top style beard ' +
    'with a small moustache at the top and a pointed tip at the bottom. Above the beard sits a pair ' +
    'of round sunglasses: frames and bridge in solid black #111111, lenses in dark grey #3A3F47. ' +
    'Reads both as a bar chart and as a bearded face. Only three colours: terracotta, black, dark grey. ' + STYLE,
};

const only = process.argv.slice(2);
const run = async () => {
  for (const [id, prompt] of Object.entries(variants)) {
    if (only.length && !only.includes(id)) continue;
    console.log(`\n▶ ${id}`);
    const taskId = await submitFluxTask({ prompt, model: 'flux-kontext-pro', aspectRatio: '1:1', outputFormat: 'png' });
    const url = await waitForFluxTask(taskId);
    const buf = await downloadFile(url);
    const file = join(OUT, `${id}.png`);
    writeFileSync(file, buf);
    console.log(`  saved ${file}  (${url})`);
  }
};
run().catch((e) => { console.error(e); process.exit(1); });
