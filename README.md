# UNIX `form` reader

A decoder for old UNIX form letters and analysis of the earliest list of UNIX
licensees.

The programs [`form(1)`](http://squoze.net/UNIX/v5man/man1/form) and [`fed(1)`](http://squoze.net/UNIX/v5man/man1/fed),
distributed with UNIX V1 to V6, were used for generating form letters from an
associative memory file and for editing such a file. As they were never ported
from PDP-11 assembly, they died off after UNIX grew to other platforms and only
a few form files survived in tape dumps.

One such form letter file is Ken Thompson's 27 June 1975 list of the
[first 51 UNIX licensees](distr/y) outside of Bell Labs! It contains multiple
iterations of letters, templated by the recipient's name and address and
formatted with roff. It was found on a [DECtape](https://www.tuhs.org/pipermail/tuhs/2023-July/028590.html)
donated to The Unix Heritage Archive by Dennis Ritchie at `ken/distr/{form.m,nmrc}`,
and first [noticed](https://www.tuhs.org/pipermail/tuhs/2023-July/028601.html)
by Jonathan Gray in 2023, just after the tapes were made public. From
[my research](https://github.com/thaliaarchi/unix-history/tree/main/lists) into
early UNIX users and history, this is the earliest list of users.

So far, this tool decodes the block headers to locate all the strings in the
associative memory. It then classifies memory by its allocation state: a string
referenced by an allocated header, memory referenced by a freed header, slack
memory between the string's length and capacity, and unclassified memory. The
associative memory file is a low level database of resizable blocks and when a
block is reallocated, the data is not cleared from the old location. Thus,
residual fragments of old versions can be recovered from the slack space between
allocated strings. For now, these residual strings are manually identified, but
I intend to automate this. Additionally, a partial ordering of writes should be
able to be determined, based on how the residual strings overlap. I have not yet
identified the relationship between keys and values.

This code is still very rough and, until the entire format has been decoded,
will not be cleaned up to decode other form files. The insights derived from
studying the [assembly source](https://github.com/dspinellis/unix-history-repo/tree/Research-V5/usr/source/s1)
are marked with comments; anything else is defensively asserted.

License: MPL-2.0
