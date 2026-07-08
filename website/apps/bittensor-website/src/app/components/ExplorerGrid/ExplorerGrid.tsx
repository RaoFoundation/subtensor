import type {ExplorerCardData} from '@/data/explorers';
import {explorers} from '@/data/explorers';
import {ExplorerCard} from '../ExplorerCard/ExplorerCard';
import styles from './ExplorerGrid.module.css';

type ExplorerGridProps = {
  cards?: ExplorerCardData[];
};

export function ExplorerGrid({cards = explorers}: ExplorerGridProps) {
  return (
    <div className={styles.grid}>
      {cards.map((card) => (
        <ExplorerCard key={card.id} card={card} />
      ))}
    </div>
  );
}
