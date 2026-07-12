import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Suspense} from 'react';
import styles from './page.module.css';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper>
        <section className={styles.section}>
          <p className={styles.paper_title}>Bittensor Explained</p>
          <p>
            There is no greater story than people&apos;s relentless and dogged endeavor to overcome
            repressive regimes. Whether we notice it or not, centralized firms, markets and
            authorities are engaged in a never-ending disempowerment of human people&apos;s
            autonomy. <strong>Bittensor</strong> is creating a new future for humanity, where new
            economies and new commodities are decentralized by design and where no single entity is
            a sole authority.
          </p>
          <p>
            At the core of the Bittensor ecosystem is the production, marketing and selling of
            digital commodities. At the expanding periphery of this ecosystem are the entire
            internet geographies of ecosystems.
          </p>
          <p>
            Everything is decentralized. Digital commodities like compute, data, storage,
            predictions, and models are transformed into intelligence. When digital commodities are
            recast as intelligence, then new architectures are discovered, new commodities are
            produced and surprisingly cheaper ways to achieve innovations are being revealed—the
            possibilities are turning out to be limitless.
          </p>
          <p>
            TAO, the decentralized currency, fuels the production of this intelligence in{' '}
            <strong>subnets.</strong>
            These intelligence-producing subnets are then innovatively connected in{' '}
            <strong>productive</strong> and <strong>profitable</strong> ways, feeding one
            intelligence into another.
          </p>
          <p>
            Entrepreneurs with skills and ideas will use Bittensor when they are deprived of
            investments from traditional sources of capital. And most important, any such
            entrepreneur can participate profitably and thrive in the Bittensor ecosystem.
          </p>
          <p>
            You can be a consumer of a subnet&apos;s digital commodity. Or if you are a
            subject-matter expert, for example an ML practitioner, then be a{' '}
            <strong>subnet miner</strong>, produce best predictions for your customer and earn TAO.
            Or, you can be a <strong>subnet validator</strong>, find markets, enterprises,
            small-businesses, application developers or end-users, for these digital products,
            generate revenue and earn TAO. Or you can just be a <strong>subnet owner</strong> and
            create fertile grounds for the growth of your subnet validators and subnet miners and
            earn TAO.
          </p>
          <p>Come join us and write your own decentralized economies into existence.</p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
