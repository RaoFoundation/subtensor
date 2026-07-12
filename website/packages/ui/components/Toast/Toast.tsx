import React, {FC} from 'react';
import {ToastContainer, toast} from 'react-toastify';

import styles from './Toast.module.css';
import 'react-toastify/dist/ReactToastify.css';

const ToastNotification: FC = () => {
  return (
    <div>
      <ToastContainer bodyClassName={styles.toaster_main} theme='dark' />
    </div>
  );
};

export {toast, ToastNotification};
