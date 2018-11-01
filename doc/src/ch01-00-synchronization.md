# Synchronization

It's possible for a centralized database to process 710,000 transactions per
second on a standard gigabit network if the transactions are, on average, no
more than 176 bytes. A centralized database can also replicate itself and
maintain high availability without significantly compromising that transaction
rate using the distributed system technique known as Optimistic Concurrency
Control [\[H.T.Kung, J.T.Robinson
(1981)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). At
Solana, we're demonstrating that these same theoretical limits apply just as
well to blockchain on an adversarial network. The key ingredient? Finding a way
to share time when nodes can't trust one-another. Once nodes can trust time,
suddenly ~40 years of distributed systems research becomes applicable to
blockchain!

> Perhaps the most striking difference between algorithms obtained by our
> method and ones based upon timeout is that using timeout produces a
> traditional distributed algorithm in which the processes operate
> asynchronously, while our method produces a globally synchronous one in which
> every process does the same thing at (approximately) the same time. Our
> method seems to contradict the whole purpose of distributed processing, which
> is to permit different processes to operate independently and perform
> different functions. However, if a distributed system is really a single
> system, then the processes must be synchronized in some way. Conceptually,
> the easiest way to synchronize processes is to get them all to do the same
> thing at the same time. Therefore, our method is used to implement a kernel
> that performs the necessary synchronization--for example, making sure that
> two different processes do not try to modify a file at the same time.
> Processes might spend only a small fraction of their time executing the
> synchronizing kernel; the rest of the time, they can operate
> independently--e.g., accessing different files. This is an approach we have
> advocated even when fault-tolerance is not required. The method's basic
> simplicity makes it easier to understand the precise properties of a system,
> which is crucial if one is to know just how fault-tolerant the system is.
> [\[L.Lamport
> (1984)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.71.1078)


