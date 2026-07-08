'use client';

import React, {FC, useEffect, useState, useRef} from 'react';
import styles from './TextArea.module.css';
import {Icon} from '../Icon/Icon';
import {Loader} from '../Loader/Loader';
import clsx from 'clsx';

export type TextAreaProps = {
  onSend: (text: string) => void;
  disabled?: boolean;
  label?: string;
};
export const TextArea: FC<TextAreaProps> = ({onSend, disabled, label}) => {
  const [val, setVal] = useState<string>('');
  const textAreaRef = useRef<HTMLTextAreaElement>(null);

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    if (textAreaRef.current === null || disabled) {
      return;
    }

    if ((textAreaRef.current.scrollHeight - 97) / 19 > 1) {
      textAreaRef.current.rows = (textAreaRef.current.scrollHeight - 97) / 19 + 1;
    } else {
      textAreaRef.current.rows = 1;
    }

    setVal(e.target.value.replace('\n', ''));
  };

  const handleSend = () => {
    const value = val.trim();

    if (value.length === 0) {
      return;
    }

    if (textAreaRef.current) {
      textAreaRef.current.rows = 1;
    }

    onSend(value);
    setVal('');
  };

  const handleKeyUp: React.DOMAttributes<HTMLTextAreaElement>['onKeyUp'] = (e) => {
    if (e.key === 'Enter' && !disabled) {
      e.preventDefault();
      handleSend();
      return false;
    }
  };

  return (
    <div className={clsx(styles.container_wrapper, disabled && styles.disabled)}>
      <div className={styles.container}>
        <textarea
          className={styles.input_box}
          placeholder={label ?? 'Start a chat'}
          ref={textAreaRef}
          onChange={onChange}
          rows={1}
          data-min-rows='1'
          value={val}
          onKeyUp={handleKeyUp}
        />
        <button className={styles.send_btn} onClick={disabled ? undefined : handleSend}>
          <Icon.Send />
        </button>
      </div>
    </div>
  );
};
