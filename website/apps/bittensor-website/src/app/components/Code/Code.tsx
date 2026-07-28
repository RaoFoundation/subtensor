'use client';

import SyntaxHighlighter from 'react-syntax-highlighter';
import syntaxStyles from './syntaxStyles';
import {Icon} from '@raofoundation/ui';
import {FC} from 'react';
import styles from './Code.module.css';

const DEFAULT_LANGUAGE = 'bash';

export type Props = {
  language?: string;
  code: string;
};

export const Code: FC<Props> = ({language = DEFAULT_LANGUAGE, code}) => {
  const handleCopyCode = () => {
    navigator.clipboard.writeText(code);
  };

  return (
    <div className={styles.code}>
      <button
        className={styles.code_copy}
        aria-label={'Copy code to clipboard'}
        onClick={handleCopyCode}
      >
        <Icon.Copy />
      </button>

      <SyntaxHighlighter language={language} style={syntaxStyles}>
        {code}
      </SyntaxHighlighter>
    </div>
  );
};
