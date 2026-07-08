'use client';

import React, {FC, useRef, useState} from 'react';
import styles from './DropDownMenu.module.css';
import clsx from 'clsx';

export type DropDownMenuProps = {
  isVisible: boolean;
  newChat?: () => void;
  close: () => void;
};
export const DropDownMenu: FC<DropDownMenuProps> = ({isVisible, newChat, close}) => {
  const RadioBtnRef = useRef<HTMLInputElement>(null);
  const [isClicked, setIsClicked] = useState(false);

  const handleNewChat = () => {
    close();
    newChat && newChat();
  };

  const handleClick = () => {
    setIsClicked(!isClicked);
    if (RadioBtnRef.current === null) return;
    else RadioBtnRef.current.checked = isClicked;
  };

  return (
    <div className={styles.wrapper}>
      <div className={clsx(styles.modal, isVisible && styles.visible)}>
        <button className={styles.new_chat_btn} onClick={handleNewChat}>
          New Chat
        </button>
        {false && (
          <div className={styles.container}>
            <p>Advanced Settings</p>
            <input type='radio' ref={RadioBtnRef} onClick={handleClick} />
          </div>
        )}
      </div>
    </div>
  );
};
