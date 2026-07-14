import React from 'react';
import {FC} from 'react';
import styles from './Input.module.css';

export type InputProps = {
  placeholder: string;
  type: string;
  inputValue: string;
  setInputValue: (value: string) => void;
};

export const Input: FC<InputProps> = ({placeholder, type, inputValue, setInputValue}) => {
  const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setInputValue(event.currentTarget.value);
  };
  return (
    <div className={styles.container}>
      <input
        type={type}
        placeholder={placeholder}
        className={styles.input}
        value={inputValue}
        onChange={handleChange}
      />
    </div>
  );
};
