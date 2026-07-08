import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Suspense} from 'react';
import {Signatures} from '../../components/Signatures/Signatures';
import styles from './page.module.css';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The Bittensor Delegates Charter</p>
        </section>
        <section className={styles.section}>
          <p>
            Bittensor itself cannot have a charter. Its core technology is a consensus mechanism,
            which reaches agreement about how its preferences should be distributed to participants
            in an open and un&minus;permissioned network. If it has preferences itself, they are
            openness and decentralization, which are immutably written into its code.
          </p>
          <p>
            As such, this document merely outlines the principles and commitments of those who use
            Bittensor as a medium to express their subjective preferences on top of its playing
            field. It is signed by The Opentensor Foundation and other entities that believe in
            Bittensor&apos;s vision of decentralized AI.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.paper_title}>Charter</p>
          <p>
            There is wide agreement that the blossoming of Artificial Intelligence offers up
            tremendous promise &minus; and risk &minus; for humanity&apos;s relationship with
            technology along multiple axes. Those include its potential use to abuse humans, long
            term existential risk to the human race, and its ability to increase power imbalances.
            We acknowledge the following principles as our commitments to stop those outcomes. Those
            are:
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>
            Our Counterpoint To Centralized Control: The greater the power, the more dangerous the
            abuse
          </p>
          <p>
            We are committed to safeguarding AI from being totally controlled or regulated by
            governments, powerful corporations, and the individuals signing this document. We
            believe that excessive centralization of AI poses the greatest risk to the human race,
            within or without Bittensor. Concentration of power will inevitably create biased
            decision&minus;making, controlled access to benefits and significant abuse. Recognizing
            that AI is the most powerful technology humanity has created to date, it is vital that
            we ensure its governance sits in the hands of the many rather than the few. To ensure
            this we are further committed to:
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>
            Decentralized Preference Consensus: The purpose of power is to give it away
          </p>
          <p>
            We firmly oppose the misuse of AI for harmful intent, and will actively strive to
            prevent the spread of harmful content. We also pledge to advocate for the positive,
            ethical and life&minus;affirming application of AI. Simultaneously, we will actively
            work to diffuse control over these preferences, in the name of decentralized power, with
            the express purpose of leveraging the collective wisdom and judgment of humanity around
            the exceedingly and increasingly difficult questions AI as a technology poses. In
            pursuit of this, we embrace:
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>
            Open Ownership: In real open source, you have the right to control your own destiny
          </p>
          <p>
            The Bittensor Network inherently allows open and un&minus;permissioned ownership accrual
            to those who contribute. We, the signatories, will work to clear the path, through which
            individuals may work to participate, and therefore gain real control in the development
            of AI. This is necessary to ensure as many humans as possible have access, influence,
            and hard power in the future that we are creating together. This principle is reinforced
            by our commitment to:
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>
            Open Source Development: For us, open&minus;source is a moral imperative
          </p>
          <p>
            We are totally committed to open&minus;source development of all of our work within the
            Bittensor ecosystem; whether it be mining, validating, subnetwork creation or any other
            value&minus;creating software, and will actively support open&minus;source development
            projects, education initiatives and those who seek to lower barriers to entry at all
            levels. We recognize the importance of collaborative efforts and community&minus;driven
            initiatives to unlock the true potential of AI; all of which can contribute to
            Bittensor. We will bolster this by upholding our value of:
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Transparency: Transparency is trust</p>
          <p>
            The Bittensor blockchain&apos;s value transfers are at all times completely transparent.
            We, the signatories, are further committed to total transparency of Bittensor&apos;s
            decision making process above and beyond what is already public, with the intention of
            making it clear what our votes in the DAO entail, and why we distribute our preferences
            in this way to direct Bittensor.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.paper_title}>Conclusion</p>
          <p>
            We have chosen Bittensor as our common platform to represent our shared values for an
            open and decentralized future of AI. We are devoted to opposing centralized control and
            encouraging decentralized decision&minus;making through open&minus;ownership, software
            development, and superior transparency. We believe together, we can shape an AI
            landscape that truly serves the collective interests of humanity
          </p>
        </section>
        <Signatures />
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
