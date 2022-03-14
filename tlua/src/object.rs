use crate::{
    AbsoluteIndex,
    AsLua,
    ffi,
    Push,
    PushOneInto,
    PushGuard,
    PushInto,
    PushResult,
    LuaError,
    LuaFunction,
    LuaState,
    LuaRead,
    LuaTable,
    Void,
};
use std::{
    convert::TryFrom,
    error::Error,
    fmt,
    num::NonZeroI32,
};

////////////////////////////////////////////////////////////////////////////////
// OnStack
////////////////////////////////////////////////////////////////////////////////

/// Types implementing this trait represent a single value stored on the lua
/// stack. Type parameter `L` represents a value guarding the state of the lua
/// stack (see [`PushGuard`]).
pub trait OnStack<L> {
    /// Get the absolute index of the value.
    fn index(&self) -> AbsoluteIndex;

    /// Get a reference to the inner stack guard.
    fn guard(&self) -> &L;

    /// Consume the value returning the inner stack guard.
    fn into_inner(self) -> L;
}

////////////////////////////////////////////////////////////////////////////////
// Index
////////////////////////////////////////////////////////////////////////////////

/// Types implementing this trait represent a single lua value that can be
/// indexed, i.e. a regular lua table or other value implementing `__index`
/// metamethod.
pub trait Index<L>: OnStack<L>
where
    L: AsLua,
{
    /// Loads a value from the table (or other object using the `__index`
    /// metamethod) given its `index`.
    ///
    /// The index must implement the [`PushOneInto`] trait and the return type
    /// must implement the [`LuaRead`] trait. See [the documentation at the
    /// crate root](index.html#pushing-and-loading-values) for more information.
    #[inline(always)]
    fn get<'lua, I, R>(&'lua self, index: I) -> Option<R>
    where
        L: 'lua,
        I: PushOneInto<LuaState>,
        I::Err: Into<Void>,
        R: LuaRead<PushGuard<&'lua L>>,
    {
        self.try_get(index).ok()
    }

    /// Loads a value from the table (or other object using the `__index`
    /// metamethod) given its `index`.
    ///
    /// # Possible errors:
    /// - `LuaError::ExecutionError` if an error happened during the check that
    ///     `index` is valid in `self`
    /// - `LuaError::WrongType` if the result lua value couldn't be read as the
    ///     expected rust type
    ///
    /// The index must implement the [`PushOneInto`] trait and the return type
    /// must implement the [`LuaRead`] trait. See [the documentation at the
    /// crate root](index.html#pushing-and-loading-values) for more information.
    #[inline]
    fn try_get<'lua, I, R>(&'lua self, index: I) -> Result<R, LuaError>
    where
        L: 'lua,
        I: PushOneInto<LuaState>,
        I::Err: Into<Void>,
        R: LuaRead<PushGuard<&'lua L>>,
    {
        imp::try_get(self.guard(), self.index(), index).map_err(|(_, e)| e)
    }

    /// Loads a value in the table (or other object using the `__index`
    /// metamethod) given its `index`, with the result capturing the table by
    /// value.
    ///
    /// See also [`Index::get`]
    #[inline(always)]
    fn into_get<I, R>(self, index: I) -> Result<R, Self>
    where
        Self: AsLua + Sized,
        I: PushOneInto<LuaState>,
        I::Err: Into<Void>,
        R: LuaRead<PushGuard<Self>>,
    {
        self.try_into_get(index).map_err(|(this, _)| this)
    }

    /// Loads a value in the table (or other object using the `__index`
    /// metamethod) given its `index`, with the result capturing the table by
    /// value.
    ///
    /// # Possible errors:
    /// - `LuaError::ExecutionError` if an error happened during the check that
    ///     `index` is valid in `self`
    /// - `LuaError::WrongType` if the result lua value couldn't be read as the
    ///     expected rust type
    ///
    /// See also [`Index::get`]
    #[inline]
    fn try_into_get<I, R>(self, index: I) -> Result<R, (Self, LuaError)>
    where
        Self: AsLua + Sized,
        I: PushOneInto<LuaState>,
        I::Err: Into<Void>,
        R: LuaRead<PushGuard<Self>>,
    {
        let this_index = self.index();
        imp::try_get(self, this_index, index)
    }

    /// Calls the method called `name` of the table (or other indexable object)
    /// with the provided `args`.
    ///
    /// Possible errors:
    /// - `MethodCallError::NoSuchMethod` in case `self[name]` is `nil`
    /// - `MethodCallError::PushError` if pushing `args` failed
    /// - `MethodCallError::LuaError` if error happened during the function call
    #[inline]
    fn call_method<'lua, A, R>(
        &'lua self,
        name: &str,
        args: A,
    ) -> Result<R, MethodCallError<A::Err>>
    where
        L: 'lua,
        Self: Push<LuaState>,
        Self::Err: Into<Void>,
        A: PushInto<LuaState>,
        R: LuaRead<PushGuard<Callable<PushGuard<&'lua L>>>>,
    {
        use MethodCallError::{NoSuchMethod, LuaError, PushError};

        self.get::<_, Callable<_>>(name)
            .ok_or(NoSuchMethod)?
            .into_call_with((self, args))
            .map_err(|e|
                match e {
                    CallError::LuaError(e) => LuaError(e),
                    CallError::PushError(e) => PushError(e.other().first()),
                }
            )
    }
}

#[derive(Debug)]
pub enum MethodCallError<E> {
    /// The corresponding method was not found (t[k] == nil)
    NoSuchMethod,
    /// Error during function call
    LuaError(LuaError),
    /// Pushing arguments failed
    PushError(E),
}

impl<E> From<CallError<E>> for MethodCallError<E> {
    fn from(e: CallError<E>) -> Self {
        match e {
            CallError::PushError(e) => Self::PushError(e),
            CallError::LuaError(e) => Self::LuaError(e),
        }
    }
}

impl<E> fmt::Display for MethodCallError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoSuchMethod => f.write_str("Method not found"),
            Self::LuaError(lua_error) => write!(f, "Lua error: {}", lua_error),
            Self::PushError(err) => {
                write!(f, "Error while pushing arguments: {}", err)
            }
        }
    }
}

impl<E> Error for MethodCallError<E>
where
    E: Error,
{
    fn description(&self) -> &str {
        match self {
            Self::NoSuchMethod => "Method not found",
            Self::LuaError(_) => "Lua error",
            Self::PushError(_) => "Error while pushing arguments",
        }
    }

    fn cause(&self) -> Option<&dyn Error> {
        match self {
            Self::NoSuchMethod => None,
            Self::LuaError(lua_error) => Some(lua_error),
            Self::PushError(err) => Some(err),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// macros
////////////////////////////////////////////////////////////////////////////////

macro_rules! impl_object {
    (
        $obj:ident,
        read($lua:ident, $index:ident) { $($read:tt)* }
        $( try_from($try_from:ident, $check:path), )*
        $( from($from:ident), )*
        $( try_into($try_into:ident, $check_into:path), )*
        $( impl $trait:ident, )*
    ) => {
        impl<L> $obj<L>
        where
            L: AsLua,
        {
            #[inline(always)]
            fn new(lua: L, index: NonZeroI32) -> Self {
                Self {
                    index: AbsoluteIndex::new(index, lua.as_lua()),
                    lua,
                }
            }
        }

        impl<L> AsLua for $obj<L>
        where
            L: AsLua,
        {
            #[inline(always)]
            fn as_lua(&self) -> LuaState {
                self.lua.as_lua()
            }
        }

        impl<L> OnStack<L> for $obj<L>
        where
            L: AsLua,
        {
            #[inline(always)]
            fn index(&self) -> AbsoluteIndex {
                self.index
            }

            #[inline(always)]
            fn guard(&self) -> &L {
                &self.lua
            }

            #[inline(always)]
            fn into_inner(self) -> L {
                self.lua
            }
        }

        $(
            impl<L> $trait<L> for $obj<L>
            where
                L: AsLua,
            {}
        )*

        impl<L> LuaRead<L> for $obj<L>
        where
            L: AsLua,
        {
            #[inline(always)]
            fn lua_read_at_position($lua: L, $index: NonZeroI32) -> Result<Self, L> {
                $( $read )*
            }
        }

        impl<L, T> Push<L> for $obj<T>
        where
            L: AsLua,
            T: AsLua,
        {
            type Err = Void;

            #[inline(always)]
            fn push_to_lua(&self, lua: L) -> PushResult<L, Self> {
                unsafe {
                    ffi::lua_pushvalue(lua.as_lua(), self.index().into());
                    Ok(PushGuard::new(lua, 1))
                }
            }
        }

        $(
            impl<L> TryFrom<$try_from<L>> for $obj<L>
            where
                L: AsLua,
            {
                type Error = $try_from<L>;

                fn try_from(other: $try_from<L>) -> Result<Self, $try_from<L>> {
                    if $check(&other.lua, other.index.0) {
                        Ok(Self { lua: other.lua, index: other.index })
                    } else {
                        Err(other)
                    }
                }
            }
        )*

        $(
            impl<L> From<$from<L>> for $obj<L>
            where
                L: AsLua,
            {
                fn from(other: $from<L>) -> Self {
                    Self { index: other.index(), lua: other.into_inner() }
                }
            }
        )*

        $(
            impl<L> TryFrom<$obj<L>> for $try_into<L>
            where
                L: AsLua,
            {
                type Error = $obj<L>;

                fn try_from(obj: $obj<L>) -> Result<Self, Self::Error> {
                    if $check_into(&obj.lua, obj.index.0) {
                        Ok(unsafe { Self::from_raw_parts(obj.lua, obj.index) })
                    } else {
                        Err(obj)
                    }
                }
            }
        )*

    }
}

////////////////////////////////////////////////////////////////////////////////
// Indexable
////////////////////////////////////////////////////////////////////////////////

/// An opaque value on lua stack that can be indexed. Can represent a lua
/// table, a lua table with a `__index` metamethod or other indexable lua
/// value.
///
/// Use this type when reading return values from lua functions or getting lua
/// function from tables.
#[derive(Debug)]
pub struct Indexable<L> {
    lua: L,
    index: AbsoluteIndex,
}

impl_object!{ Indexable,
    read(lua, index) {
        if imp::is_indexable(&lua, index) {
            Ok(Self::new(lua, index))
        } else {
            Err(lua)
        }
    }
    try_from(Callable, imp::is_indexable),
    from(IndexableRW),
    from(LuaTable),
    try_into(LuaTable, imp::is_table),
    impl Index,
}

////////////////////////////////////////////////////////////////////////////////
// NewIndex
////////////////////////////////////////////////////////////////////////////////

/// Types implementing this trait represent a single lua value that can be
/// changed by indexed, i.e. a regular lua table or other value implementing
/// `__newindex` metamethod.
pub trait NewIndex<L>: OnStack<L>
where
    L: AsLua,
{
    /// Inserts or modifies a `value` of the table (or other object using the
    /// `__index` or `__newindex` metamethod) given its `index`.
    ///
    /// Contrary to [`checked_set`], can only be called when writing the key and
    /// value cannot fail (which is the case for most types).
    ///
    /// # Panic
    ///
    /// Will panic if an error happens during attempt to set value. Can happen
    /// if `__index` or `__newindex` throws an error. Use [`try_set`] if this
    /// is a possibility in your case.
    ///
    /// The index must implement the [`PushOneInto`] trait and the return type
    /// must implement the [`LuaRead`] trait. See [the documentation at the
    /// crate root](index.html#pushing-and-loading-values) for more information.
    #[inline(always)]
    fn set<I, V>(&self, index: I, value: V)
    where
        I: PushOneInto<LuaState>, I::Err: Into<Void>,
        V: PushOneInto<LuaState>, V::Err: Into<Void>,
    {
        if let Err(e) = self.try_set(index, value) {
            panic!("Setting value failed: {}", e)
        }
    }

    /// Inserts or modifies a `value` of the table (or other object using the
    /// `__index` or `__newindex` metamethod) given its `index`.
    ///
    /// Contrary to [`try_checked_set`], can only be called when writing the key
    /// and value cannot fail (which is the case for most types).
    ///
    /// Returns a `LuaError::ExecutionError` in case an error happened during an
    /// attempt to set value.
    ///
    /// The index must implement the [`PushOneInto`] trait and the return type
    /// must implement the [`LuaRead`] trait. See [the documentation at the
    /// crate root](index.html#pushing-and-loading-values) for more information.
    #[inline]
    fn try_set<I, V>(&self, index: I, value: V) -> Result<(), LuaError>
    where
        I: PushOneInto<LuaState>, I::Err: Into<Void>,
        V: PushOneInto<LuaState>, V::Err: Into<Void>,
    {
        imp::try_checked_set(self.guard(), self.index(), index, value)
            .map_err(|e|
                match e {
                    Ok(_) => unreachable!("Void is uninstantiatable"),
                    Err(e) => e,
                }
            )
    }

    /// Inserts or modifies a `value` of the table (or other object using the
    /// `__newindex` metamethod) given its `index`.
    ///
    /// Returns an error if pushing `index` or `value` failed. This can only
    /// happen for a limited set of types. You are encouraged to use the [`set`]
    /// method if pushing cannot fail.
    ///
    /// # Panic
    ///
    /// Will panic if an error happens during attempt to set value. Can happen
    /// if `__index` or `__newindex` throws an error. Use [`try_checked_set`] if
    /// this is a possibility in your case.
    #[inline(always)]
    fn checked_set<I, V>(
        &self,
        index: I,
        value: V,
    ) -> Result<(), CheckedSetError<I::Err, V::Err>>
    where
        I: PushOneInto<LuaState>,
        V: PushOneInto<LuaState>,
    {
        self.try_checked_set(index, value)
            .map_err(|e| e.unwrap_or_else(|e| panic!("Setting value failed: {}", e)))
    }

    /// Inserts or modifies a `value` of the table (or other object using the
    /// `__newindex` metamethod) given its `index`.
    ///
    /// # Possible errors
    /// - Returns an error if pushing `index` or `value` failed. This can only
    /// happen for a limited set of types. You are encouraged to use the [`set`]
    /// method if pushing cannot fail.
    /// - Returns a `LuaError::ExecutionError` in case an error happened during
    /// an attempt to set value.
    #[inline(always)]
    fn try_checked_set<I, V>(
        &self,
        index: I,
        value: V,
    ) -> Result<(), TryCheckedSetError<I::Err, V::Err>>
    where
        I: PushOneInto<LuaState>,
        V: PushOneInto<LuaState>,
    {
        imp::try_checked_set(self.guard(), self.index(), index, value)
    }
}

pub type TryCheckedSetError<K, V> = Result<CheckedSetError<K, V>, LuaError>;

/// Error returned by the [`NewIndex::checked_set`] function.
#[derive(Debug, Copy, Clone)]
pub enum CheckedSetError<K, V> {
    /// Error while pushing the key.
    KeyPushError(K),
    /// Error while pushing the value.
    ValuePushError(V),
}

////////////////////////////////////////////////////////////////////////////////
// IndexableRW
////////////////////////////////////////////////////////////////////////////////

/// An opaque value on lua stack that can be indexed immutably as well as
/// mutably. Can represent a lua table, a lua table with a `__index` and
/// `__newindex` metamethods or other indexable lua value.
///
/// Use this type when reading return values from lua functions or getting lua
/// function from tables.
#[derive(Debug)]
pub struct IndexableRW<L> {
    lua: L,
    index: AbsoluteIndex,
}

impl_object!{ IndexableRW,
    read(lua, index) {
        if imp::is_rw_indexable(&lua, index) {
            Ok(Self::new(lua, index))
        } else {
            Err(lua)
        }
    }
    try_from(Indexable, imp::is_rw_indexable),
    try_from(Callable, imp::is_rw_indexable),
    from(LuaTable),
    try_into(LuaTable, imp::is_table),
    impl Index,
    impl NewIndex,
}

////////////////////////////////////////////////////////////////////////////////
// Call
////////////////////////////////////////////////////////////////////////////////

pub trait Call<L>: OnStack<L>
where
    L: AsLua,
{
    #[inline]
    fn call<'lua, R>(&'lua self) -> Result<R, LuaError>
    where
        L: 'lua,
        R: LuaRead<PushGuard<&'lua L>>,
    {
        Ok(self.call_with(())?)
    }

    #[inline]
    fn call_with<'lua, A, R>(&'lua self, args: A) -> Result<R, CallError<A::Err>>
    where
        L: 'lua,
        A: PushInto<LuaState>,
        R: LuaRead<PushGuard<&'lua L>>,
    {
        imp::call(self.guard(), self.index(), args)
    }

    #[inline]
    fn into_call<R>(self) -> Result<R, LuaError>
    where
        Self: AsLua + Sized,
        R: LuaRead<PushGuard<Self>>,
    {
        Ok(self.into_call_with(())?)
    }

    #[inline]
    fn into_call_with<A, R>(self, args: A) -> Result<R, CallError<A::Err>>
    where
        Self: AsLua + Sized,
        A: PushInto<LuaState>,
        R: LuaRead<PushGuard<Self>>,
    {
        let index = self.index();
        imp::call(self, index, args)
    }
}

/// Error that can happen when calling a type implementing [`Call`].
#[derive(Debug)]
pub enum CallError<E> {
    /// Error while executing the function.
    LuaError(LuaError),
    /// Error while pushing one of the parameters.
    PushError(E),
}

impl<E> From<LuaError> for CallError<E> {
    fn from(e: LuaError) -> Self {
        Self::LuaError(e)
    }
}

impl<E> From<CallError<E>> for LuaError
where
    E: Into<Void>,
{
    fn from(e: CallError<E>) -> Self {
        match e {
            CallError::LuaError(le) => le,
            CallError::PushError(_) => {
                unreachable!("no way to create instance of Void")
            }
        }
    }
}

impl<E> fmt::Display for CallError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::LuaError(lua_error) => write!(f, "Lua error: {}", lua_error),
            Self::PushError(err) => {
                write!(f, "Error while pushing arguments: {}", err)
            }
        }
    }
}

impl<E> Error for CallError<E>
where
    E: Error,
{
    fn description(&self) -> &str {
        match self {
            Self::LuaError(_) => "Lua error",
            Self::PushError(_) => "error while pushing arguments",
        }
    }

    fn cause(&self) -> Option<&dyn Error> {
        match self {
            Self::LuaError(lua_error) => Some(lua_error),
            Self::PushError(err) => Some(err),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Callable
////////////////////////////////////////////////////////////////////////////////

/// An opaque value on lua stack that can be called. Can represent a lua
/// function, a lua table with a `__call` metamethod or other callable lua
/// value.
///
/// Use this type when reading return values from lua functions or getting lua
/// function from tables.
#[derive(Debug)]
pub struct Callable<L> {
    lua: L,
    index: AbsoluteIndex,
}

impl_object!{ Callable,
    read(lua, index) {
        if imp::is_callable(&lua, index) {
            Ok(Self::new(lua, index))
        } else {
            Err(lua)
        }
    }
    try_from(Indexable, imp::is_callable),
    try_from(IndexableRW, imp::is_callable),
    from(LuaFunction),
    try_into(LuaFunction, imp::is_function),
    impl Call,
}

////////////////////////////////////////////////////////////////////////////////
// imp
////////////////////////////////////////////////////////////////////////////////

mod imp {
    use crate::{
        AbsoluteIndex,
        AsLua,
        c_ptr,
        error,
        ffi,
        PushGuard,
        PushInto,
        PushOneInto,
        LuaError,
        LuaState,
        LuaRead,
        ToString,
        Void,
    };
    use super::{
        CallError,
        CheckedSetError,
        TryCheckedSetError,
    };
    use std::num::NonZeroI32;

    pub(super) fn try_get<T, I, R>(
        this: T,
        this_index: AbsoluteIndex,
        index: I,
    ) -> Result<R, (T, LuaError)>
    where
        T: AsLua,
        I: PushOneInto<LuaState>,
        I::Err: Into<Void>,
        R: LuaRead<PushGuard<T>>,
    {
        let raw_lua = this.as_lua();
        unsafe {
            // push index onto the stack
            raw_lua.push_one(index).assert_one_and_forget();
            // move index into registry
            let index_ref = ffi::luaL_ref(raw_lua, ffi::LUA_REGISTRYINDEX);
            // push indexable onto the stack
            ffi::lua_pushvalue(raw_lua, this_index.into());
            // move indexable into registry
            let table_ref = ffi::luaL_ref(raw_lua, ffi::LUA_REGISTRYINDEX);

            let res = protected_call(raw_lua, |l| {
                // push indexable
                ffi::lua_rawgeti(l, ffi::LUA_REGISTRYINDEX, table_ref);
                // push index
                ffi::lua_rawgeti(l, ffi::LUA_REGISTRYINDEX, index_ref);
                // pop index, push value
                ffi::lua_gettable(l, -2);
                // save value
                ffi::luaL_ref(l, ffi::LUA_REGISTRYINDEX)
                // stack is temporary so indexable is discarded after return
            });
            let value_ref = match res {
                Ok(value_ref) => value_ref,
                Err(e) => return Err((this, e)),
            };

            // move value from registry to stack
            ffi::lua_rawgeti(raw_lua, ffi::LUA_REGISTRYINDEX, value_ref);
            let res = R::lua_read(PushGuard::new(this, 1))
                .map_err(|g| {
                    let e = LuaError::wrong_type_returned::<R, _>(raw_lua, 1);
                    (g.into_inner(), e)
                });

            // unref temporaries
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, value_ref);
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, index_ref);
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, table_ref);

            res
        }
    }

    pub(super) fn try_checked_set<T, I, V>(
        this: T,
        this_index: AbsoluteIndex,
        index: I,
        value: V,
    ) -> Result<(), TryCheckedSetError<I::Err, V::Err>>
    where
        T: AsLua,
        I: PushOneInto<LuaState>,
        V: PushOneInto<LuaState>,
    {
        let raw_lua = this.as_lua();
        unsafe {
            // push value onto the stack
            raw_lua.try_push_one(value)
                .map_err(|(e, _)| Ok(CheckedSetError::ValuePushError(e)))?
                .assert_one_and_forget();
            // move value into registry
            let value_ref = ffi::luaL_ref(raw_lua, ffi::LUA_REGISTRYINDEX);

            // push index onto the stack
            raw_lua.try_push_one(index)
                .map_err(|(e, _)| Ok(CheckedSetError::KeyPushError(e)))?
                .assert_one_and_forget();
            // move index into registry
            let index_ref = ffi::luaL_ref(raw_lua, ffi::LUA_REGISTRYINDEX);

            // push indexable onto the stack
            ffi::lua_pushvalue(raw_lua, this_index.into());
            // move indexable into registry
            let table_ref = ffi::luaL_ref(raw_lua, ffi::LUA_REGISTRYINDEX);

            protected_call(raw_lua, |l| {
                // push indexable
                ffi::lua_rawgeti(l, ffi::LUA_REGISTRYINDEX, table_ref);
                // push index
                ffi::lua_rawgeti(l, ffi::LUA_REGISTRYINDEX, index_ref);
                // push value
                ffi::lua_rawgeti(l, ffi::LUA_REGISTRYINDEX, value_ref);
                // pop index, push value
                ffi::lua_settable(l, -3);
                // stack is temporary so indexable is discarded after return
            })
            .map_err(Err)?;

            // unref temporaries
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, value_ref);
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, index_ref);
            ffi::luaL_unref(raw_lua, ffi::LUA_REGISTRYINDEX, table_ref);

            Ok(())
        }
    }

    fn protected_call<L, F, R>(lua: L, f: F) -> Result<R, LuaError>
    where
        L: AsLua,
        F: FnOnce(LuaState) -> R,
    {
        let mut ud = PCallCtx { r#in: Some(f), out: None };
        let ud_ptr = &mut ud as *mut PCallCtx<_, _>;
        let rc = unsafe {
            ffi::lua_cpcall(lua.as_lua(), trampoline::<F, R>, ud_ptr.cast())
        };
        match rc {
            0 => {}
            ffi::LUA_ERRMEM => panic!("lua_cpcall returned LUA_ERRMEM"),
            ffi::LUA_ERRRUN => unsafe {
                let error_msg = ToString::lua_read(PushGuard::new(lua, 1))
                    .ok()
                    .expect("can't find error message at the top of the Lua stack");
                return Err(LuaError::ExecutionError(error_msg.into()))
            }
            rc => panic!("Unknown error code returned by lua_cpcall: {}", rc),
        }
        return Ok(ud.out.expect("if trampoline succeeded the value is set"));

        struct PCallCtx<F, R> {
            r#in: Option<F>,
            out: Option<R>,
        }

        unsafe extern "C" fn trampoline<F, R>(l: LuaState) -> i32
        where
            F: FnOnce(LuaState) -> R,
        {
            let ud_ptr = ffi::lua_touserdata(l, 1);
            let PCallCtx { r#in, out } = ud_ptr.cast::<PCallCtx::<F, R>>()
                .as_mut()
                .unwrap_or_else(|| error!(l, "userdata is null"));

            let f = r#in.take().expect("callback must be set by caller");
            out.replace(f(l));

            0
        }
    }

    #[inline]
    pub(super) fn call<T, A, R>(
        this: T,
        index: AbsoluteIndex,
        args: A,
    ) -> Result<R, CallError<A::Err>>
    where
        T: AsLua,
        A: PushInto<LuaState>,
        R: LuaRead<PushGuard<T>>,
    {
        let raw_lua = this.as_lua();
        // calling pcall pops the parameters and pushes output
        let (pcall_return_value, pushed_value) = unsafe {
            let old_top = ffi::lua_gettop(raw_lua);
            // lua_pcall pops the function, so we have to make a copy of it
            ffi::lua_pushvalue(raw_lua, index.into());
            let num_pushed = match this.as_lua().try_push(args) {
                Ok(g) => g.forget_internal(),
                Err((err, _)) => return Err(CallError::PushError(err)),
            };
            let pcall_return_value = ffi::lua_pcall(
                raw_lua,
                num_pushed,
                ffi::LUA_MULTRET,
                0,
            );
            let n_results = ffi::lua_gettop(raw_lua) - old_top;
            (pcall_return_value, PushGuard::new(this, n_results))
        };

        match pcall_return_value {
            ffi::LUA_ERRMEM => panic!("lua_pcall returned LUA_ERRMEM"),
            ffi::LUA_ERRRUN => {
                let error_msg = ToString::lua_read(pushed_value)
                    .ok()
                    .expect("can't find error message at the top of the Lua stack");
                return Err(LuaError::ExecutionError(error_msg.into()).into())
            }
            0 => {}
            _ => panic!("Unknown error code returned by lua_pcall: {}", pcall_return_value),
        }

        let n_results = pushed_value.size;
        LuaRead::lua_read_at_maybe_zero_position(pushed_value, -n_results)
            .map_err(|lua| LuaError::wrong_type_returned::<R, _>(lua, n_results).into())
    }

    #[inline(always)]
    pub(super) fn is_callable(lua: impl AsLua, index: NonZeroI32) -> bool {
        let raw_lua = lua.as_lua();
        let i = index.into();
        unsafe {
            // luaL_iscallable doesn't work for `ffi`
            if ffi::lua_isfunction(raw_lua, i) {
                true
            } else if ffi::luaL_getmetafield(raw_lua, i, c_ptr!("__call")) != 0 {
                // Pop the metafield
                ffi::lua_pop(raw_lua, 1);
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    pub(super) fn is_indexable(lua: impl AsLua, index: NonZeroI32) -> bool {
        let raw_lua = lua.as_lua();
        let i = index.into();
        unsafe {
            if ffi::lua_istable(raw_lua, i) {
                true
            } else if ffi::luaL_getmetafield(raw_lua, i, c_ptr!("__index")) != 0 {
                // Pop the metafield
                ffi::lua_pop(raw_lua, 1);
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    pub(super) fn is_rw_indexable(lua: impl AsLua, index: NonZeroI32) -> bool {
        let raw_lua = lua.as_lua();
        let i = index.into();
        unsafe {
            let oldtop = ffi::lua_gettop(raw_lua);
            if ffi::lua_istable(raw_lua, i) {
                true
            } else if
                ffi::luaL_getmetafield(raw_lua, i, c_ptr!("__index")) != 0
                && ffi::luaL_getmetafield(raw_lua, i, c_ptr!("__newindex")) != 0
            {
                // Pop the metafields
                ffi::lua_settop(raw_lua, oldtop);
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    pub(super) fn is_table(lua: impl AsLua, index: NonZeroI32) -> bool {
        unsafe { ffi::lua_istable(lua.as_lua(), index.into()) }
    }

    #[inline(always)]
    pub(super) fn is_function(lua: impl AsLua, index: NonZeroI32) -> bool {
        unsafe { ffi::lua_isfunction(lua.as_lua(), index.into()) }
    }
}