TEST(operation, geogCRS_to_geogCRS_context_incompatible_area) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("4267"), // NAD27
        authFactory->createCoordinateReferenceSystem("4258"), // ETRS 89
        ctxt);
    ASSERT_EQ(list.size(), 1U);
    EXPECT_EQ(list[0]->exportToPROJString(PROJStringFormatter::create().get()),
              "+proj=noop");
}

