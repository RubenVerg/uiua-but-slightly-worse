# ...but slightly worse

This page serves as a list of deltas between Uiua and Uiua but slightly worse. Does this variant deserve the moniker, or should it be called Uiua but slightly better?

## Lambdas

Lambdas are by far the most powerful addition to Uiua but slightly worse. Syntactically, they only add two glyphs: the monadic modifier [`lambda`](/docs/lambda) and the dyadic function [`call`](/docs/call). `lambda` always returns a niladic function which evaulates to a value that contains the function it was given as an operand. As this is a normal value, you can put it in an array. To turn it back into a function, you use `call`. However, Uiua functions and therefore Uiua but slightly worse functions require a fixed signature. You might want to type `call 3 4 lambda add`, but also `call 10 lambda negate`; as you can see in the first case `call` is taking three arguments and in the second it's taking two. As `call` doesn't know what lambda it will receive at compile time, I had to make a choice that sacrificed a little bit of usability. Said choice is to accept the arguments as a box array. Similarly, because Uiua functions can return more than one thing, the results are also in a box array. The previous examples are actually

```uiua
°□₁⋌□₂3 4⋋+
```

and

```
°□₁⋌□₁10⋋¯
```

Is this ugly? Yes. Is this needed? Kinda...? Sometimes you *do* know the signature of a lambda, even if the compiler doesn't; consider

```uiua
F ← ˜⊏[⋋+⋋-]
```

where both functions are `|2.1`, no matter the argument. You can tell [`call`](/docs/call) how many arguments a function takes by using a subscripts. The previous examples therefore become this:

```uiua
⋌₂3 4⋋+
⋌₁10⋋¯
```

Pretty nice. But! I just told you that some functions return more than one thing! How do we tell `call` *that*? There's no nice way to give two subscripts to a function other than separating them with a sided subscript, which is cursed but at least easy to type. Remember that [`un`](/docs/un)[`multiply`](/docs/multiply) is a `|1.2` function.

```uiua
⋌₁⌞₂¯10⋋°×
```

Lambdas also support certain arithmetic operations. [`multiply`](/docs/multiply)ing two lambdas composes them; taking the [`reciprocal`](/docs/reciprocal) of a lambda inverts it; taking the [`power`](/docs/power) of a lambda to a number repeats it.

## Superscripts

In Uiua, certain functions like [`add`](/docs/add) accept a subscript which binds the first argument; for example `+₁` is the function that increments. This is useful when you need to pass it to a modifier, for example [`both`](/docs/both). However, sometimes you want to bind a number to a function that doesn't get this special type of subscript, for example [`select`](/docs/select); `(⊏1)` is a perfectly reasonable thing to want as an unit without those pesky brackets. Similarly, you may have your own function `Binomial` defined, and you want to bind a number to *it*. Uiua but slightly worse introduces *superscripts*, the visual dual to subscripts. A superscript can be put to the right of **any** function, and (binding tightly) it binds the superscripted number to the function. This works with functions of all types and arity. To type a superscript, put `*` before a number.

```
+²   # Like +2, but as an unit (equivalent to +₂ in Uiua)
√²   # Like √2, but as an unit
√₃²  # Like √₃2, but as an unit
Foo ← +
Foo² # Like Foo 2, but as an unit
```

In fact, why limit it to just functions? There's a bit of a hole in the syntax as it is now: while applying a modifier to a bound function is now easy, binding an already modified function isn't, and you have to use ugly parentheses again. What if the superscript was *on the modifier itself*? Doing that allows you to bind an argument *after* the modifier is applied.

```
Triad ← ++
◡Triad² # ◡(Triad 2)
◡²Triad # (◡Triad 2)
```

Pretty nice.

Superscripts also allow for signed infinities; you type them with `*inf` or `*<backtick>inf` and they format to... `ºº`. Not my greatest work.

## Bind

Sometimes, you want to bind something that's not a number to a function. Uiua used to have a feature for this called `bind`. I brought it back with the modifier [`bind`](/docs/bind). Of course it's not limited to nilads, and binding any two functions puts them next to each other as an unit.

## Identity subscripts

Very simply, subscripted [`identity`](/docs/identity) creates an identity function that takes that many arguments.

```uiua
Sig! ←^ ⋅⊢

Sig!∸
Sig!∸₂
Sig!∸₈
```

Why does [`identity`](/docs/identity) look like that? You'll see later.

## Infinity subscripts

Just like infinity superscripts, there's also infinity subscripts, you type them as you'd expect, and they format to `ₒₒ`. Currently, the only function to use them is [`keep`](/docs/keep), which extends the keep subscript "keep along n axes" to keep along *all* axes.

## Ylpitlum

This is just the beginning of the things with weird names.

```uiua
⇌$ multiply
```

[`ylpitlum`](/docs/ylpitlum) is a "conceptual inverse" to [`multiply`](/docs/multiply). When used with numbers between zero and one, multiply treats them as probabilities of independent events and gives you the probability that both happen at once (P(A ∩ B) = P(A)P(B)). Ylpitlum gives you the probability that at least one of them happens, which is the inverse of the probability that neither of them happens, i.e. `¬×∩¬`, which is exactly the definition of ylpitlum.

## Pag and Drawkcab

The trend continues, see if you can figure out what these reverse to.

[`pag`](/docs/pag) is a "conceptual invere" to [`gap`](/docs/gap) and [`drawkcab`](/docs/drawkcab) is a "conceptual inverse" to [`backward`](/docs/backward). "Conceptual inverse" evidently has no objective meaning, but there is no objectivity in the slightly worse universe, there are only my decisions. [`gap`](/docs/gap) is, in some sense, pre-pop: `gap F` is always equal to `bind F pop`. Pag is instead post-pop: `pag F` is always equal to `bind pop F`. Similarly, drawkcab applies the same operation that [`backward`](/docs/backward) does to the inputs of a function, but to the outputs: if there's two, it swaps them; if there's four, it does the funky thing.

## Repeat subscripts

"Doesn't repeat already have subscripts?" Yes, but they're not as awesome as they can be. If you *want* the old behavior, of course, you can use a superscript on [`repeat`](/docs/repeat), as discussed above. The new subscripts are much more powerful though: because we know the repeat count at compile time, we can actually repeat the operand that requested amount of time, with no signature problems!

```uiua
⍥₃+ 1 2 3 4
```

And the old behavior for comparison:

```uiua
⍥³+ 1 2
```

## LCM and GCD

Uiua contains a function named `or` which takes the GCD of two numbers. In Uiua but slightly worse, this function is renamed to [`gcd`](/docs/gcd), and a companion [`lcm`](/docs/lcm) function is added.

Why does [`lcm`](/docs/lcm) have the `fold` glyph? You have a keen eye(;.

## Fold glyph change

To make space for [`lcm`](/docs/lcm), [`fold`](/docs/fold) has changed glyph. I like this one more anyways.

## Rank

[`rank`](/docs/rank) is just a quick shortcut for [`length`](/docs/length)[`shape`](/docs/shape). Why? To avoid brackets with `°⊸⧊`, of course. Since `ran` formats to [`range`](/docs/range), you can write `rnk` for a three letter shorthand.

## Roolf, Gniliec and Dnuor

The reversed name series has a new episode. Comes with a third meaning of "conceptual inverse"!

[`absolute value`](/docs/absolute%20value) is a "lossy" function. It discards the sign of the argument and just returns its magnitude. Its dual, in some sense, is [`sign`](/docs/sign), because we can recover the whole number from the absolute value and the sign. [`floor`](/docs/floor) is also a lossy function: it discards the fractional part of the argument. However, there is no dual primitive to recover it; until now. [`roolf`](/docs/roolf) is a "fractional part" primitive. [`gniliec`](/docs/gniliec) and [`dnuor`](/docs/dnuor) are the corresponding functions for [`ceiling`](/docs/ceiling) and [`round`](/docs/round). In all three cases, adding the results of the two duals recovers the original argument.

## Now glyph

Really simple: [`now`](/docs/now) gets its own pretty glyph, a stylized hourglass.

## Identity glyph change and One

Nowadays, Uiua has the primitive [`reciprocal`](/docs/reciprocal), but it used not to, and while it was being suggested as an addition, two glyphs kept being suggested: `⨪` and `∸`, both with the argument that a reciprocal is a "half division", in some sense. I argued that the former glyph should be chosen: if we treat the glyph for [divide](/docs/divide) as a stylized fraction, the two dots represent places where the two arguments go. When taking the reciprocal of a number, we put it in the bottom slot of the fraction, and assume that the top one is `1`. In the same discussion, a joke was brought up: if `⨪` is a reciprocal, what's `∸`? By the same analogy, if we assume the top slot is where the argument goes, and the bottom slot is filled with `1`, it's `÷1 x`, i.e. the identity function! Following this joke, Uiua but slightly worse changes the [`identity`](/docs/identity) glyph, and goes one step further: what if we remove all the dots? We get `÷1 1`, which is... [`one`](/docs/one)? I guess there's a glyph for the number one now. Don't ask, I don't know either.
