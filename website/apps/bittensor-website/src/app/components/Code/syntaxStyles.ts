import {CSSProperties} from 'react';
import {atelierSulphurpoolDark} from 'react-syntax-highlighter/dist/cjs/styles/hljs';

const Colors = {
  lightBlue: '#008017',
  deepBlue: '#1b252d',
  warmGray: '#aaaaaa',
  lightGreen: '#7ddc91',
};

const syntaxStyles = {
  ...atelierSulphurpoolDark,

  hljs: {
    display: 'flex',
    overflowX: 'auto',
    background: 'rgba(5, 4, 4, 0.05)',
    color: '#050404',
    padding: '16px 64px 16px 24px',
    fontFamily: 'FiraCode',
    fontWeight: '400',
    fontSize: '12px',
    lineHeight: '140%',
  },

  'hljs-variable': {
    color: Colors.lightBlue,
  },
  'hljs-template-variable': {
    color: Colors.lightBlue,
  },
  'hljs-attribute': {
    color: Colors.lightBlue,
  },
  'hljs-tag': {
    color: Colors.lightBlue,
  },
  'hljs-name': {
    color: Colors.lightBlue,
  },
  'hljs-regexp': {
    color: Colors.lightBlue,
  },
  'hljs-link': {
    color: Colors.lightBlue,
  },
  'hljs-selector-id': {
    color: Colors.lightBlue,
  },
  'hljs-selector-class': {
    color: Colors.lightBlue,
  },

  'hljs-comment': {
    color: Colors.warmGray,
  },
  'hljs-quote': {
    color: Colors.warmGray,
  },

  'hljs-number': {
    color: Colors.lightBlue,
  },
  'hljs-meta': {
    color: Colors.lightBlue,
  },
  'hljs-built_in': {
    color: Colors.lightBlue,
  },
  'hljs-builtin-name': {
    color: Colors.lightBlue,
  },
  'hljs-literal': {
    color: Colors.lightBlue,
  },
  'hljs-type': {
    color: Colors.lightBlue,
  },
  'hljs-params': {
    color: Colors.lightBlue,
  },

  'hljs-keyword': {
    color: Colors.lightBlue,
  },
  'hljs-selector-tag': {
    color: Colors.lightBlue,
  },

  'hljs-string': {
    color: Colors.lightGreen,
  },
  'hljs-symbol': {
    color: Colors.lightGreen,
  },
  'hljs-bullet': {
    color: Colors.lightGreen,
  },
} as {[key: string]: CSSProperties};

export default syntaxStyles;
