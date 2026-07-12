'use client';
import {Link} from '@raofoundation/ui';
import {useEffect, useState} from 'react';
import styles from './Signatures.module.css';

export type RawSignature = {
  name: string;
  signature: string;
  url: string;
};

export type Signature = RawSignature & {
  key: string;
};

export type RawSignaturesResponse = Record<string, RawSignature>;

const isSignature = (signature: any): signature is RawSignature => {
  return (
    typeof signature.name === 'string' &&
    typeof signature.signature === 'string' &&
    typeof signature.url === 'string'
  );
};

export const Signatures = () => {
  const url = 'https://raw.githubusercontent.com/RaoFoundation/bittensor-charter/main/signatures.json';

  const [signatures, setSignatures] = useState<Signature[]>([]);

  useEffect(() => {
    const fetchSignatures = async () => {
      const response = await fetch(url);
      const data = await response.json();
      const listOfSignatures: Signature[] = [];
      Object.entries(data).forEach(([key, signature]) => {
        if (isSignature(signature)) {
          listOfSignatures.push({...signature, key});
        }
      });

      setSignatures(listOfSignatures);
    };

    fetchSignatures();
  }, []);

  return signatures.length > 0 ? (
    <div className={styles.container}>
      <p className={styles.title}>Signed by</p>
      <div className={styles.delegates_container}>
        {signatures.map((signature, idx) => {
          const prefix = idx < signatures.length - 1 ? '\u00A0\u00A0•\u00A0\u00A0' : '';
          signature.url = signature.url.startsWith('http')
            ? signature.url
            : `https://${signature.url}`;
          return (
            <div key={signature.key}>
              <Link href={signature.url} target='_blank' rel='noreferrer'>
                {signature.name}
              </Link>
              {prefix}
            </div>
          );
        })}
      </div>
      <br />
      <Link
        href={'https://github.com/RaoFoundation/bittensor-charter/'}
        target='_blank'
        rel='noreferrer'
        style={{fontSize: '12px'}}
      >
        How to verify signatures?
      </Link>
    </div>
  ) : null;
};
