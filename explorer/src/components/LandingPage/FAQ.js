import React, {useState} from 'react'
import { Container, Row, Col, Media, Button, Collapse, Card, CardBody} from 'reactstrap'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';

export const FAQ = () => {

  const [isOpen_1, setIsOpen_1] = useState(false);
  const [isOpen_2, setIsOpen_2] = useState(false);
  const [isOpen_3, setIsOpen_3] = useState(false);
  const [isOpen_4, setIsOpen_4] = useState(false);
  const [isOpen_5, setIsOpen_5] = useState(false);
  const [isOpen_6, setIsOpen_6] = useState(false);
  const [isOpen_7, setIsOpen_7] = useState(false);
  const [isOpen_8, setIsOpen_8] = useState(false);
  const [isOpen_9, setIsOpen_9] = useState(false);
  const toggle_1 = () => setIsOpen_1(!isOpen_1);
  const toggle_2 = () => setIsOpen_2(!isOpen_2);
  const toggle_3 = () => setIsOpen_3(!isOpen_3);
  const toggle_4 = () => setIsOpen_4(!isOpen_4);
  const toggle_5 = () => setIsOpen_5(!isOpen_5);
  const toggle_6 = () => setIsOpen_6(!isOpen_6);
  const toggle_7 = () => setIsOpen_7(!isOpen_7);
  const toggle_8 = () => setIsOpen_8(!isOpen_8);
  const toggle_9 = () => setIsOpen_9(!isOpen_9);

  const questions = [
    {
      question: "What is Solana?",
      answer: (<div><p><a href="https://solana.com/"  target="_blank">Solana</a> is a high performance blockchain built for web scale. Their <a href="https://solana.com/team"  target="_blank">team</a> has years of experience in scaling distributed systems and so, they were able to solve the "Blockchain Trilemma". The term was initially coined by Vitalik Buterin and refers to the difficulty of designing a scalable, secure and decentralized blockchain. Before Solana, scaling a blockchain ment you either had to compromise on security or decentralization.</p>
               <p>Solana leverages <a href="https://medium.com/solana-labs/7-innovations-that-make-solana-the-first-web-scale-blockchain-ddc50b1defda"  target="_blank">eight core innovations</a> in order to provide a scalable and secure public blockchain without sacrificing decentralization. It's a protocol that aims to scale out on a single blockchain. No sharding, no sidechains, no fancy off-chain scaling solutions. Currently, the network supports up to 65.000 TPS at 400 ms blocktime with a globally distributed validator set of 50 nodes.</p></div>),
      collapse: [isOpen_1, toggle_1],
    },
    {
      question:"Why Solana?",
      answer:(<div>
              <p>A platform that is not used, has no use. Todays blockchain suffer from low throughput, long confirmation times and in general, a bad developer / user experience. This directly translates into low user numbers, which in turn leads to low adoption - a self-reinforcing cycle that hinders blockchain technology from reaching mass adoption. </p>
              <p>Solana built a blockchain that feels like a traditional web2 solution. Meaning, fast blocktimes (400 ms), near instant finality and high throughput (up to 65k TPS in it's current form). Since Solana scales out on one single blockchain, developers and users alike also profit from <a href="https://a16z.com/2018/12/16/4-eras-of-blockchain-computing-degrees-of-composability/"  target="_blank">composability</a>. A high degree of composability facilitates the emergence of a variety of ecosystems to be built on top of Solana. Interested in building? Check out the <a href="https://docs.solana.com/"  target="_blank">Solana Docs</a> and their <a href="https://solana.com/accelerator"  target="_blank">Accelerator Program.</a></p>
              </div>),
      collapse: [isOpen_2, toggle_2],
    },
    {
      question:"What is the \"Staker Price Guarantee\"? ",
      answer: (<div>
                  <p>The "Staker Price Guarantee" is offered to EVERY tokenholder, but has two requirements: </p>
                  <p> <li>registration (KYC) on <a href="http://coinlist.co"  target="_blank">coinlist.co</a> within 3 months after the auction clears on March 24th, 2020</li></p>
                  <p> <li>continuous staking during the time of registration and redemption (minimum time: 2 days)</li></p>
                  <p>The "Staker Price Guarantee" is redeemable at any time for 12 months from the start of the registration period. If you fullfill the two requirements, the "Staker Price Guarantee" ensures you a buyback guarantee for 90% of the auction clearing price. </p>
                  <p>For more, check out: <a href="https://solana.com/staker-price-guarantee.pdf"  target="_blank">https://solana.com/staker-price-guarantee.pdf</a></p>
               </div>),
      collapse: [isOpen_3, toggle_3],
    },
    {
      question:"Where can I buy tokens?",
      answer: (<div>
                  <p>You can trade SOL on <a href="https://www.binance.com/en/trade/SOL_BTC"  target="_blank">Binance.</a> </p>
                  <p>Get an overview of all available markets <a href="https://coinmarketcap.com/currencies/solana/markets/"  target="_blank">here.</a></p>
              </div>),
      collapse: [isOpen_4, toggle_4],
    },
    {
      question:"I am a validator, how can I add my identity to the validator list above?",
      answer: (<div>
                  <p>Publish your information on the Solana blockchain with <span className="code">solana validator-info publish</span>. We will pick you up from there on a daily basis. The image will be pulled from your keybase profile.</p>
              </div>),
      collapse: [isOpen_5, toggle_5],
    },
  ]

  let i = 0;

  return (

    <div className="pb-5">
      <Container id="faq" className="py-5">
        <Media>
          <Media body>

            <div className="text-center py-5"><h3 className="display-3 mb-2 font-weight-bold">FAQs</h3></div>

            {questions.map(question => (

              <div className="container font-size-md">
                <Row className="questionOuter">
                  <Col xs="12" sm="12" md="12" lg="12" className="d-flex justify-content-between px-3 py-3 question" onClick={question.collapse[1]}>
                    <div className="">{i=i+1}</div>
                    <div className="text-center">{question.question}</div>
                    <div className="">{question.collapse[0] == false ? <FontAwesomeIcon className="primary-blue rounded-circle hover-pic" icon={['fas', 'plus']} /> : <FontAwesomeIcon className="primary-blue rounded-circle hover-pic" icon={['fas', 'minus']} />}</div>
                  </Col>
                  <Collapse isOpen={question.collapse[0]} className="w-100">
                    <div className="answerOuter">

                      <div className="answerInner">
                        <Row>
                          <Col xs="12" sm="12" md="10" lg="10" className="answerText">
                            {question.answer}
                          </Col>
                        </Row>
                      </div>

                    </div>
                  </Collapse>
                </Row>
              </div>

            ))}

          </Media>

        </Media>

      </Container>
    </div>
  )
}


