# Predefined functions

## Math

### Basics

* `fn abs<T>(x: T) -> T`
* `fn round<T>(x: T) -> T`
* `fn floor<T>(x: T) -> T`
* `fn ceil<T>(x: T) -> T`
* `fn mod<T>(a: T, b: T) -> T`
* `fn sqrt<D>(x: D^2) -> D`
* `fn sqr<D>(x: D) -> D^2`

### Exponential and logarithm

* `fn exp(x: Scalar) -> Scalar`
* `fn ln(x: Scalar) -> Scalar`
* `fn log(x: Scalar) -> Scalar`
* `fn log10(x: Scalar) -> Scalar`
* `fn log2(x: Scalar) -> Scalar`

### Trigonometry

Basic:

* `fn cos(x: Scalar) -> Scalar`
* `fn sin(x: Scalar) -> Scalar`
* `fn tan(x: Scalar) -> Scalar`
* `fn asin(x: Scalar) -> Scalar`
* `fn acos(x: Scalar) -> Scalar`
* `fn atan(x: Scalar) -> Scalar`
* `fn atan2<T>(y: T, x: T) -> Scalar`

Hyperbolic:

* `fn sinh(x: Scalar) -> Scalar`
* `fn cosh(x: Scalar) -> Scalar`
* `fn tanh(x: Scalar) -> Scalar`
* `fn asinh(x: Scalar) -> Scalar`
* `fn acosh(x: Scalar) -> Scalar`
* `fn atanh(x: Scalar) -> Scalar`

When importing the `math::trigonometry_extra` module (`use math::trigonometry_extra`),
you also get access to:

* `fn cot(x: Scalar) -> Scalar`
* `fn acot(x: Scalar) -> Scalar`
* `fn coth(x: Scalar) -> Scalar`
* `fn acoth(x: Scalar) -> Scalar`
* `fn secant(x: Scalar) -> Scalar`
* `fn arcsecant(x: Scalar) -> Scalar`
* `fn cosecant(x: Scalar) -> Scalar`
* `fn csc(x: Scalar) -> Scalar`
* `fn acsc(x: Scalar) -> Scalar`
* `fn sech(x: Scalar) -> Scalar`
* `fn asech(x: Scalar) -> Scalar`
* `fn csch(x: Scalar) -> Scalar`
* `fn acsch(x: Scalar) -> Scalar`

### Others

* `fn gamma(x: Scalar) -> Scalar`

### Statistics

* `fn mean<D>(xs: D…) -> D`
* `fn maximum<D>(xs: D…) -> D`
* `fn minimum<D>(xs: D…) -> D`

### Geometry

* `fn hypot2<T>(x: T, y: T) -> T`
* `fn hypot3<T>(x: T, y: T, z: T) -> T`
* `fn circle_area<L>(radius: L) -> L^2`
* `fn circle_circumference<L>(radius: L) -> L`
* `fn sphere_area<L>(radius: L) -> L^2`
* `fn sphere_volume<L>(radius: L) -> L^3`

## Physics

### Temperature conversion

* `fn from_celsius(t_celsius: Scalar) -> Temperature`
* `fn to_celsius(t_kelvin: Temperature) -> Scalar`
* `fn from_fahrenheit(t_fahrenheit: Scalar) -> Temperature`
* `fn to_fahrenheit(t_kelvin: Temperature) -> Scalar`

## Strings

* `fn str_length(s: str) -> Scalar`
* `fn str_slice(s: str, start: Scalar, end: Scalar) -> str`
* `fn str_append(a: str, b: str) -> str`
* `fn str_contains(haystack: str, needle: str) -> bool`
* `fn str_replace(s: str, pattern: str, replacement: str) -> str`
* `fn str_repeat(a: str, n: Scalar) -> str`
