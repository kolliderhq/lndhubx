use err_derive::Error;

#[derive(Debug, Error)]
pub enum GeneralError {
    #[error(display = "Migrations failed to run")]
    MigrationError(#[source] diesel_migrations::RunMigrationsError),
    #[error(display = "You've attempted to run migrations in a production environment,\
                       this is disallowed")]
    MigrationsInProd,
}
