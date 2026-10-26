import type { ThemeRegistrationRaw } from 'shiki';

/**
 * Grayscale Shiki themes for the monochrome site. Syntax is carried by weight and tone instead of hue: keywords
 * are bold at full contrast, strings and numbers sit one step back, punctuation two, and comments are italic
 * at the lightest readable gray. Used by MDX code blocks (source.config.ts) and the marketing pages.
 */
function monochrome(type: 'light' | 'dark'): ThemeRegistrationRaw {
  const light = type === 'light';
  const tone = light
    ? {
        fg: '#0a0a0a',
        strong: '#000000',
        string: '#525252',
        literal: '#404040',
        type: '#262626',
        punctuation: '#737373',
        comment: '#8f8f8f',
        bg: '#fafafa',
      }
    : {
        fg: '#e5e5e5',
        strong: '#ffffff',
        string: '#a3a3a3',
        literal: '#d4d4d4',
        type: '#f5f5f5',
        punctuation: '#8a8a8a',
        comment: '#6f6f6f',
        bg: '#171717',
      };

  return {
    name: `worktree-mono-${type}`,
    type,
    colors: {
      'editor.background': tone.bg,
      'editor.foreground': tone.fg,
    },
    settings: [
      { settings: { foreground: tone.fg, background: tone.bg } },
      {
        scope: ['comment', 'punctuation.definition.comment', 'string.comment'],
        settings: { foreground: tone.comment, fontStyle: 'italic' },
      },
      {
        scope: [
          'keyword',
          'storage',
          'storage.type',
          'storage.modifier',
          'keyword.operator.new',
          'keyword.operator.expression',
          'keyword.control',
          'variable.language',
          'constant.language.boolean',
          'constant.language.null',
          'constant.language.undefined',
        ],
        settings: { foreground: tone.strong, fontStyle: 'bold' },
      },
      {
        scope: [
          'string',
          'string.quoted',
          'string.template',
          'punctuation.definition.string',
          'markup.inline.raw',
        ],
        settings: { foreground: tone.string },
      },
      {
        scope: ['constant.numeric', 'constant.language', 'constant.character', 'constant.other'],
        settings: { foreground: tone.literal },
      },
      {
        scope: [
          'entity.name.function',
          'support.function',
          'meta.function-call entity.name.function',
          'entity.name.tag',
        ],
        settings: { foreground: tone.fg, fontStyle: 'bold' },
      },
      {
        scope: [
          'entity.name.type',
          'entity.name.class',
          'support.type',
          'support.class',
          'entity.other.inherited-class',
          'entity.name.namespace',
        ],
        settings: { foreground: tone.type, fontStyle: 'italic' },
      },
      {
        scope: [
          'variable',
          'variable.other',
          'variable.parameter',
          'meta.object-literal.key',
          'support.type.property-name',
          'entity.other.attribute-name',
        ],
        settings: { foreground: tone.fg },
      },
      {
        scope: [
          'punctuation',
          'meta.brace',
          'keyword.operator',
          'punctuation.separator',
          'punctuation.terminator',
          'meta.delimiter',
        ],
        settings: { foreground: tone.punctuation },
      },
      { scope: ['markup.heading', 'markup.bold'], settings: { fontStyle: 'bold' } },
      { scope: ['markup.italic'], settings: { fontStyle: 'italic' } },
      { scope: ['markup.inserted'], settings: { foreground: tone.strong } },
      {
        scope: ['markup.deleted'],
        settings: { foreground: tone.comment, fontStyle: 'strikethrough' },
      },
    ],
  };
}

export const codeThemes = {
  light: monochrome('light'),
  dark: monochrome('dark'),
};
